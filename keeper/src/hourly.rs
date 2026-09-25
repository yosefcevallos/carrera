//! Hourly pass (spec §12): record inputs, evaluate the rule, move loans while the
//! market is open, size up, process exit epochs, crystallise fees, refresh NAV.
//! Every crank is best-effort; a failure is logged and the pass moves on. The
//! program is the authority on what is allowed.

use crate::venue_accounts::VenueArgs;
use crate::{
    accounts::{OverlayVault, VaultState},
    calendar,
    config::{MarketOpenMode, VaultCfg},
    ix::REASON_RULE,
    rule::{self, Decision, Inputs},
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
    let (mb, ms) = ctx.venues.lock().await.rates();
    if supplies && (mb.is_none() || ms.is_none()) {
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
    let price = ctx.venues.lock().await.price(sym);
    let can_refresh = price.is_some() || !ctx.venues.lock().await.supplies_values();
    if !can_refresh {
        tracing::warn!("{sym}: no price value, skipping refresh_nav");
    }

    // Fresh price before anything that reads it.
    if can_refresh {
        chain.try_send(&format!("{sym} refresh_nav"), chain.ix.refresh_nav(&vault, &vc.oracle, price)).await;
    }
    let mut v = chain.vault(&vc.mint).await?;

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
            finish_wind(ctx, sym, &vault, v.step).await;
        }
        VaultState::Unwinding => {
            finish_unwind(ctx, sym, &vault, v.step).await;
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
            apply(ctx, sym, &vault, state, d).await;
        }
    }

    // 4. Size up after deposits, in either deployed mode.
    let v = chain.vault(&vc.mint).await?;
    if let Ok(s) = v.state() {
        if matches!(s, VaultState::Parked | VaultState::Basis) && v.market_open {
            let ltv = v.ltv_bps(vc.stock_decimals);
            if ltv + v.params.size_band_bps < v.params.ltv_bps {
                chain.try_send(&format!("{sym} size_up (ltv {ltv})"), chain.ix.size_up(&vault, &VenueArgs::none())).await;
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
        chain.try_send(&format!("{sym} refresh_nav"), chain.ix.refresh_nav(&vault, &vc.oracle, price)).await;
    }
    Ok(())
}

async fn apply(ctx: &Ctx, sym: &str, vault: &Pubkey, state: VaultState, d: Decision) {
    let chain = &ctx.chain;
    match (state, d) {
        (_, Decision::None) => {}
        (VaultState::Parked | VaultState::Idle, Decision::ToBasis) => {
            // From Idle the program may require `park` first; if wind_start is refused we
            // fall back to parking and try again next hour.
            if chain.try_send(&format!("{sym} wind_start"), chain.ix.wind_start(vault, &VenueArgs::none())).await {
                finish_wind(ctx, sym, vault, 0).await;
            } else if state == VaultState::Idle {
                chain.try_send(&format!("{sym} park"), chain.ix.park(vault, &VenueArgs::none())).await;
            }
        }
        (VaultState::Basis, Decision::ToParked) => {
            if chain.try_send(&format!("{sym} unwind_start(rule)"), chain.ix.unwind_start(vault, REASON_RULE)).await {
                finish_unwind(ctx, sym, vault, 0).await;
            }
        }
        (VaultState::Basis, Decision::ToIdle) => {
            if chain.try_send(&format!("{sym} unwind_start(rule)"), chain.ix.unwind_start(vault, REASON_RULE)).await
                && finish_unwind(ctx, sym, vault, 0).await
            {
                chain.try_send(&format!("{sym} repay"), chain.ix.repay(vault, &VenueArgs::none())).await;
            }
        }
        (VaultState::Parked, Decision::ToIdle) => {
            chain.try_send(&format!("{sym} repay"), chain.ix.repay(vault, &VenueArgs::none())).await;
        }
        (VaultState::Idle, Decision::ToParked) => {
            chain.try_send(&format!("{sym} park"), chain.ix.park(vault, &VenueArgs::none())).await;
        }
        (s, d) => tracing::debug!("{sym}: no crank for {:?} in {}", d, s.name()),
    }
}

/// Continue a wind from `done` completed steps through commit. Returns true on commit.
async fn finish_wind(ctx: &Ctx, sym: &str, vault: &Pubkey, done: u8) -> bool {
    let chain = &ctx.chain;
    for n in (done + 1)..=3 {
        if !chain.try_send(&format!("{sym} wind_step({n})"), chain.ix.wind_step(vault, n, &VenueArgs::none())).await {
            return false;
        }
    }
    chain.try_send(&format!("{sym} wind_commit"), chain.ix.wind_commit(vault, &VenueArgs::none())).await
}

pub(crate) async fn finish_unwind(ctx: &Ctx, sym: &str, vault: &Pubkey, done: u8) -> bool {
    let chain = &ctx.chain;
    for n in (done + 1)..=3 {
        if !chain.try_send(&format!("{sym} unwind_step({n})"), chain.ix.unwind_step(vault, n, &VenueArgs::none())).await {
            return false;
        }
    }
    chain.try_send(&format!("{sym} unwind_commit"), chain.ix.unwind_commit(vault, &VenueArgs::none())).await
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
                chain.try_send(&format!("{sym} settle_epoch({prev})"), chain.ix.settle_epoch(vault, prev, &vc.mint, &vc.stock_token_program, &VenueArgs::none())).await;
            }
        }
    }
    let due = Utc::now().timestamp() >= v.epoch_opened_ts.saturating_add(v.params.epoch_len_secs as i64);
    if v.pending_exit_shares > 0 && due {
        let id = v.epoch_id;
        if chain.try_send(&format!("{sym} close_epoch({id})"), chain.ix.close_epoch(vault, id)).await {
            chain.try_send(&format!("{sym} settle_epoch({id})"), chain.ix.settle_epoch(vault, id, &vc.mint, &vc.stock_token_program, &VenueArgs::none())).await;
        }
    }
}
