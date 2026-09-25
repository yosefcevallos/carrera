//! Hourly pass (spec §12): record inputs, evaluate the rule, move loans while the
//! market is open, size up, process exit epochs, crystallise fees, refresh NAV.
//! Every crank is best-effort; a failure is logged and the pass moves on. The
//! program is the authority on what is allowed.

use crate::{
    accounts::{OverlayVault, VaultState},
    calendar,
    config::{MarketOpenMode, ProgramBuild, VaultCfg},
    ix::REASON_RULE,
    rule::{self, Decision, Inputs},
    venue::{self, Swap},
    Ctx,
};
use anyhow::Result;
use chrono::Utc;
use solana_sdk::pubkey::Pubkey;

pub async fn run_once(ctx: &Ctx) -> Result<()> {
    let r = pass(ctx).await;
    let new_alerts = ctx.alerts.lock().await.drain();
    let mut st = ctx.status.write().await;
    for a in new_alerts {
        st.alert(a.level, a.vault.as_deref(), a.message);
    }
    st.keeper.last_hourly_run_ts = crate::status::now_ts();
    st.keeper.hourly_ok = r.is_ok();
    r
}

async fn pass(ctx: &Ctx) -> Result<()> {
    ctx.venues.lock().await.reload().await;
    let chain = &ctx.chain;
    let supplies = ctx.venues.lock().await.supplies_values();

    // 1. Inputs: funding per vault, Kamino rates once. When the keeper supplies the
    // values (live / mock) and has none for a vault, the crank is skipped rather than
    // sent with a zero.
    for vc in &ctx.cfg.vaults {
        let vault = chain.pdas().vault(&vc.mint);
        let mock = ctx.venues.lock().await.funding(&vc.symbol);
        if supplies && mock.is_none() {
            tracing::warn!("{}: no funding value, skipping record_funding", vc.symbol);
            continue;
        }
        chain.try_send(&format!("{} record_funding", vc.symbol), chain.ix.record_funding(&vault, &vc.hawkeye_view, mock)).await;
    }
    let (mb, ms) = if ctx.cfg.program_build == ProgramBuild::Real { (None, None) } else { ctx.venues.lock().await.rates() };
    if ctx.cfg.program_build == ProgramBuild::Mock && supplies && (mb.is_none() || ms.is_none()) {
        tracing::warn!("no Kamino rates value, skipping record_kamino_rates");
    } else {
        chain.try_send("record_kamino_rates", chain.ix.record_kamino_rates(&ctx.cfg.kamino_reserve, mb, ms)).await;
    }

    let registry = chain.registry().await?;
    let nyse_open = calendar::market_open(Utc::now());

    for vc in &ctx.cfg.vaults {
        // Auto: stricter of the NYSE cash session and Phoenix's own market state (live feed).
        // Open / Closed force the on-chain flag (config `market_open`; Open bypasses spec §7.5).
        let open_now = match ctx.cfg.market_open {
            MarketOpenMode::Auto => nyse_open && ctx.venues.lock().await.phoenix_open(&vc.symbol).unwrap_or(true),
            MarketOpenMode::Open => true,
            MarketOpenMode::Closed => false,
        };
        if let Err(e) = vault_pass(ctx, vc, registry.borrow_apy_bps, registry.supply_apy_bps, registry.paused, open_now).await {
            tracing::warn!("{}: hourly pass aborted: {e:#}", vc.symbol);
        }
    }

    let sol = chain.sol_balance().await.unwrap_or(0.0);
    ctx.status.write().await.keeper.sol_balance = sol;
    ctx.alerts.lock().await.check_sol(sol, ctx.cfg.min_keeper_sol).await;
    Ok(())
}

async fn vault_pass(ctx: &Ctx, vc: &VaultCfg, borrow_bps: u32, supply_bps: u32, paused: bool, open_now: bool) -> Result<()> {
    let chain = &ctx.chain;
    let sym = &vc.symbol;
    let vault = chain.pdas().vault(&vc.mint);
    let real = ctx.cfg.program_build == ProgramBuild::Real;
    let price = if real { None } else { ctx.venues.lock().await.price(sym) };
    let can_refresh = real || price.is_some() || !ctx.venues.lock().await.supplies_values();
    if !can_refresh {
        tracing::warn!("{sym}: no price value, skipping refresh_nav");
    }

    // Real build: obligation, Phoenix trader and token account exist before any leg runs.
    if let Err(e) = venue::ensure_setup(ctx, vc).await {
        tracing::warn!("{sym}: venue setup: {e:#}");
    }

    // Fresh price before anything that reads it.
    if can_refresh {
        refresh_nav(ctx, vc, price, "refresh_nav").await;
    }
    let mut v = chain.vault(&vc.mint).await?;

    // Real build: stock deposited since the last pass goes into the obligation.
    if real {
        match venue::custody_balance(chain, vc).await {
            Ok(held) if held > 0 => {
                venue::send_crank(ctx, vc, &v, Swap::None, &format!("{sym} sync_collateral({held})"), |a| chain.ix.sync_collateral(&vault, a)).await;
                v = chain.vault(&vc.mint).await?;
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("{sym}: custody read: {e:#}"),
        }
    }

    // 2. Market flag.
    if v.market_open != open_now
        && chain.try_send(&format!("{sym} set_market_open({open_now})"), chain.ix.set_market_open(&vault, open_now)).await
    {
        v.market_open = open_now;
    }

    // 3. Rule and transitions.
    let state = v.state()?;
    match state {
        VaultState::Winding => {
            finish_wind(ctx, vc, v.step).await;
        }
        VaultState::Unwinding => {
            finish_unwind(ctx, vc, v.step).await;
        }
        _ => {
            let inputs = Inputs {
                state,
                f_avg_bps: v.f_avg_bps(),
                samples: v.funding_samples,
                supply_bps,
                borrow_bps,
                market_open: v.market_open,
                paused,
            };
            let d = rule::evaluate(&v.params, &inputs);
            let h = rule::hurdles(&v.params, supply_bps, borrow_bps);
            tracing::info!(
                "{sym}: {} f_avg={} hurdle_parked={} hurdle_idle={} carry_ok={} → {:?}",
                state.name(), inputs.f_avg_bps, h.from_parked_bps, h.from_idle_bps, h.carry_ok, d
            );
            apply(ctx, vc, &v, state, d).await;
        }
    }

    // 4. Size up after deposits, in either deployed mode.
    let v = chain.vault(&vc.mint).await?;
    if let Ok(s) = v.state() {
        if matches!(s, VaultState::Parked | VaultState::Basis) && v.market_open {
            let ltv = v.ltv_bps(vc.stock_decimals);
            if ltv + v.params.size_band_bps < v.params.ltv_bps {
                let swap = venue::swap_for_size_up(&v, vc.stock_decimals);
                venue::send_crank(ctx, vc, &v, swap, &format!("{sym} size_up (ltv {ltv})"), |a| chain.ix.size_up(&vault, a)).await;
            }
        }
    }

    // 5. Exit epochs.
    epochs(ctx, vc, &vault, &v).await;

    // 6. Fees, then a final NAV refresh so the cache is fresh for deposits.
    if let Some(t) = ctx.cfg.treasury_shares {
        let v = chain.vault(&vc.mint).await?;
        if v.share_price_stock_e6 > v.high_water_e6 {
            chain.try_send(&format!("{sym} crystallise_fee"), chain.ix.crystallise_fee(&vault, &t)).await;
        }
    }
    if can_refresh {
        refresh_nav(ctx, vc, price, "refresh_nav").await;
    }
    Ok(())
}

/// `refresh_nav` for either build: the mock build takes the keeper's price, the real build
/// reads the xStock reserve after refreshing it.
pub(crate) async fn refresh_nav(ctx: &Ctx, vc: &VaultCfg, price: Option<u64>, label: &str) -> bool {
    let chain = &ctx.chain;
    let vault = chain.pdas().vault(&vc.mint);
    if ctx.cfg.program_build == ProgramBuild::Real {
        match venue::refresh_nav_accounts(chain, &ctx.cfg, vc).await {
            Ok((reserve, extra)) => chain.try_send(&format!("{} {label}", vc.symbol), chain.ix.refresh_nav_with(&vault, &reserve, None, extra)).await,
            Err(e) => {
                tracing::warn!("{}: {label}: {e:#}", vc.symbol);
                false
            }
        }
    } else {
        chain.try_send(&format!("{} {label}", vc.symbol), chain.ix.refresh_nav(&vault, &vc.oracle, price)).await
    }
}

async fn apply(ctx: &Ctx, vc: &VaultCfg, v: &OverlayVault, state: VaultState, d: Decision) {
    let chain = &ctx.chain;
    let sym = &vc.symbol;
    let vault = chain.pdas().vault(&vc.mint);
    match (state, d) {
        (_, Decision::None) => {}
        (VaultState::Parked | VaultState::Idle, Decision::ToBasis) => {
            // From Idle the program may require `park` first; if wind_start is refused we
            // fall back to parking and try again next hour.
            if venue::send_crank(ctx, vc, v, Swap::None, &format!("{sym} wind_start"), |a| chain.ix.wind_start(&vault, a)).await {
                finish_wind(ctx, vc, 0).await;
            } else if state == VaultState::Idle {
                venue::send_crank(ctx, vc, v, Swap::None, &format!("{sym} park"), |a| chain.ix.park(&vault, a)).await;
            }
        }
        (VaultState::Basis, Decision::ToParked) => {
            if chain.try_send(&format!("{sym} unwind_start(rule)"), chain.ix.unwind_start(&vault, REASON_RULE)).await {
                finish_unwind(ctx, vc, 0).await;
            }
        }
        (VaultState::Basis, Decision::ToIdle) => {
            if chain.try_send(&format!("{sym} unwind_start(rule)"), chain.ix.unwind_start(&vault, REASON_RULE)).await
                && finish_unwind(ctx, vc, 0).await
            {
                venue::send_crank_fresh(ctx, vc, |_| Swap::None, &format!("{sym} repay"), |a| chain.ix.repay(&vault, a)).await;
            }
        }
        (VaultState::Parked, Decision::ToIdle) => {
            venue::send_crank(ctx, vc, v, Swap::None, &format!("{sym} repay"), |a| chain.ix.repay(&vault, a)).await;
        }
        (VaultState::Idle, Decision::ToParked) => {
            venue::send_crank(ctx, vc, v, Swap::None, &format!("{sym} park"), |a| chain.ix.park(&vault, a)).await;
        }
        (s, d) => tracing::debug!("{sym}: no crank for {:?} in {}", d, s.name()),
    }
}

/// Continue a wind from `done` completed steps through commit. Returns true on commit.
/// Each step re-reads the vault so the swap size follows the previous step's outcome.
async fn finish_wind(ctx: &Ctx, vc: &VaultCfg, done: u8) -> bool {
    let chain = &ctx.chain;
    let sym = &vc.symbol;
    let vault = chain.pdas().vault(&vc.mint);
    for n in (done + 1)..=3 {
        if !venue::send_crank_fresh(ctx, vc, |v| venue::swap_for_wind_step(v, n), &format!("{sym} wind_step({n})"), |a| chain.ix.wind_step(&vault, n, a)).await {
            return false;
        }
    }
    venue::send_crank_fresh(ctx, vc, |_| Swap::None, &format!("{sym} wind_commit"), |a| chain.ix.wind_commit(&vault, a)).await
}

pub(crate) async fn finish_unwind(ctx: &Ctx, vc: &VaultCfg, done: u8) -> bool {
    let chain = &ctx.chain;
    let sym = &vc.symbol;
    let vault = chain.pdas().vault(&vc.mint);
    for n in (done + 1)..=3 {
        if !venue::send_crank_fresh(ctx, vc, |v| venue::swap_for_unwind_step(v, n), &format!("{sym} unwind_step({n})"), |a| chain.ix.unwind_step(&vault, n, a)).await {
            return false;
        }
    }
    venue::send_crank_fresh(ctx, vc, |_| Swap::None, &format!("{sym} unwind_commit"), |a| chain.ix.unwind_commit(&vault, a)).await
}

/// Settle any closed-but-unsettled previous epoch, then close and settle the current
/// one once its window has elapsed and it holds shares.
async fn epochs(ctx: &Ctx, vc: &VaultCfg, vault: &Pubkey, v: &OverlayVault) {
    let chain = &ctx.chain;
    let sym = &vc.symbol;
    if v.epoch_id > 0 {
        let prev = v.epoch_id - 1;
        if let Ok(Some(e)) = chain.epoch(vault, prev).await {
            if e.closed && !e.settled {
                venue::send_crank(ctx, vc, v, Swap::None, &format!("{sym} settle_epoch({prev})"), |a| chain.ix.settle_epoch(vault, prev, &vc.mint, &vc.stock_token_program, a)).await;
            }
        }
    }
    let due = Utc::now().timestamp() >= v.epoch_opened_ts.saturating_add(v.params.epoch_len_secs as i64);
    if v.pending_exit_shares > 0 && due {
        let id = v.epoch_id;
        if chain.try_send(&format!("{sym} close_epoch({id})"), chain.ix.close_epoch(vault, id)).await {
            venue::send_crank_fresh(ctx, vc, |_| Swap::None, &format!("{sym} settle_epoch({id})"), |a| chain.ix.settle_epoch(vault, id, &vc.mint, &vc.stock_token_program, a)).await;
        }
    }
}
