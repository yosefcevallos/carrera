//! 60-second pass (spec §6.3, §12): read LTV and Phoenix margin per vault, fire
//! rebalances or emergency actions, and raise alerts.

use crate::{accounts::VaultState, config::ProgramBuild, ix::REASON_EMERGENCY, status::Rates, venue::{self, Swap}, Ctx};
use anyhow::Result;

/// Fast-loop price freshness bound for the live feed.
const PRICE_MAX_AGE_SECS: i64 = 300;

pub async fn run_once(ctx: &Ctx) -> Result<()> {
    let r = pass(ctx).await;
    let new_alerts = ctx.alerts.lock().await.drain();
    let mut st = ctx.status.write().await;
    for a in new_alerts {
        st.alert(a.level, a.vault.as_deref(), a.message);
    }
    st.keeper.last_fast_run_ts = crate::status::now_ts();
    st.keeper.fast_ok = r.is_ok();
    r
}

async fn pass(ctx: &Ctx) -> Result<()> {
    let chain = &ctx.chain;
    let slot = chain.slot().await?;
    let reg = chain.registry().await?;
    let rates = Rates { borrow_bps: reg.borrow_apy_bps, supply_bps: reg.supply_apy_bps, paused: reg.paused };
    ctx.status.write().await.keeper.registry_paused = reg.paused;
    // Live feed: make sure prices exist and are under 5 minutes old so NAV refreshes
    // work right after a restart instead of waiting for the first hourly reload.
    ctx.venues.lock().await.ensure_prices(PRICE_MAX_AGE_SECS).await;
    for vc in &ctx.cfg.vaults {
        let sym = &vc.symbol;
        let vault = chain.pdas().vault(&vc.mint);
        let mut v = match chain.vault(&vc.mint).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("{sym}: read failed: {e:#}");
                continue;
            }
        };

        // Keep the NAV cache well inside the program's staleness bound so deposits
        // between hourly passes do not fail with NavStale. Threshold is one third of
        // the vault's own `max_nav_age_slots`; price comes from the same lookup the
        // hourly pass uses (last feed snapshot).
        let nav_age = slot.saturating_sub(v.nav_slot);
        let refresh_after = v.params.max_nav_age_slots / 3;
        if v.params.max_nav_age_slots > 0 && nav_age > refresh_after {
            let real = ctx.cfg.program_build == ProgramBuild::Real;
            let (price, supplies) = {
                let ven = ctx.venues.lock().await;
                (if real { None } else { ven.price(sym) }, ven.supplies_values())
            };
            if real || price.is_some() || !supplies {
                tracing::info!("{sym}: NAV {nav_age} slots old (> {refresh_after}), refreshing");
                if crate::hourly::refresh_nav(ctx, vc, price, "refresh_nav (fast)").await {
                    if let Ok(fresh) = chain.vault(&vc.mint).await {
                        v = fresh;
                    }
                }
            } else {
                tracing::warn!("{sym}: NAV {nav_age} slots old but no price value, skipping refresh_nav");
            }
        }
        let feed = ctx.venues.lock().await.feed_view(sym);
        // Real build: margin and health use the trader account's live equity, not the cached field.
        let live = if ctx.cfg.program_build == crate::config::ProgramBuild::Real {
            match venue::live_equity(ctx, &vault).await {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("{sym}: live Phoenix equity read failed: {e:#}");
                    None
                }
            }
        } else {
            None
        };
        {
            let mut st = ctx.status.write().await;
            st.record_vault(sym, &v, vc.stock_decimals, rates, slot, live);
            st.set_feed(sym, feed);
        }
        ctx.alerts.lock().await.check_vault(sym, &v, vc.stock_decimals, slot).await;
        let state = match v.state() {
            Ok(s) => s,
            Err(_) => continue,
        };
        let p = &v.params;
        let ltv = v.ltv_bps(vc.stock_decimals);
        let margin = match live {
            Some(e) => v.margin_bps_with(vc.stock_decimals, e.withdrawable_usdc()),
            None => v.margin_bps(vc.stock_decimals),
        };

        // Emergency first: skips the rule, keeps slippage bounds (program side).
        let ltv_emergency = p.emergency_ltv_bps > 0 && ltv > p.emergency_ltv_bps;
        let margin_emergency = margin.map(|m| m < p.min_margin_bps).unwrap_or(false);
        match state {
            VaultState::Basis if ltv_emergency || margin_emergency => {
                chain.try_send(&format!("{sym} unwind_start(emergency) ltv={ltv} margin={margin:?}"), chain.ix.unwind_start(&vault, REASON_EMERGENCY)).await;
                continue;
            }
            VaultState::Parked if ltv_emergency => {
                venue::send_crank(ctx, vc, &v, Swap::None, venue::NEED_K, &format!("{sym} repay (emergency) ltv={ltv}"), |a| chain.ix.repay(&vault, a)).await;
                continue;
            }
            _ => {}
        }

        match state {
            VaultState::Basis => {
                if ltv > p.ltv_bps + p.rebalance_ltv_band_bps {
                    venue::send_crank(ctx, vc, &v, Swap::None, venue::NEED_KP, &format!("{sym} rebalance_to_kamino ltv={ltv}"), |a| chain.ix.rebalance_to_kamino(&vault, a)).await;
                } else if let Some(m) = margin {
                    if m < p.min_margin_bps + p.rebalance_margin_band_bps {
                        venue::send_crank(ctx, vc, &v, Swap::None, venue::NEED_KP, &format!("{sym} rebalance_to_phoenix margin={m}"), |a| chain.ix.rebalance_to_phoenix(&vault, a)).await;
                    }
                }
            }
            VaultState::Parked => {
                if ltv > p.ltv_bps + p.rebalance_ltv_band_bps {
                    let amount = v.debt_excess_usdc(vc.stock_decimals).min(v.parked_usdc);
                    if amount > 0 {
                        venue::send_crank(ctx, vc, &v, Swap::None, venue::NEED_K, &format!("{sym} rebalance_from_parked({amount}) ltv={ltv}"), |a| chain.ix.rebalance_from_parked(&vault, amount, a)).await;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}
