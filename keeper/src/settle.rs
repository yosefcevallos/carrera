//! Settlement pass, every `settle_interval_secs` (default 5 min). For each vault
//! whose open exit epoch holds shares and whose window has elapsed, free the
//! liability (unwind / repay as the state requires), then `close_epoch` and
//! `settle_epoch`, so exits settle without waiting for the hourly pass.
//!
//! Sequence per vault:
//!   refresh NAV if older than a third of `max_nav_age_slots`
//!   Unwinding → resume `unwind_step` from the current step, `unwind_commit`
//!   Basis     → `unwind_partial(fraction, ExitDemand)` when the exit is smaller than
//!               the whole position, else `unwind_start(ExitDemand)` + steps + commit
//!   Parked    → `repay`
//!   Idle with residual `debt_usdc` → `repay` (accepted from Idle after the hotfix)
//!   then `close_epoch`, `settle_epoch`
//! Every send is best-effort; the program is the authority. One log line per vault.

use crate::{
    accounts::VaultState,
    config::VaultCfg,
    hourly::finish_unwind,
    ix::REASON_EXIT_DEMAND,
    Ctx,
};
use anyhow::Result;
use chrono::Utc;

/// Share of the position leaving in this epoch, in bps, capped at 10 000.
pub fn exit_fraction_bps(pending_exit_shares: u64, total_shares: u64) -> u32 {
    if total_shares == 0 || pending_exit_shares >= total_shares {
        return 10_000;
    }
    (pending_exit_shares as u128 * 10_000 / total_shares as u128) as u32
}

pub async fn run_once(ctx: &Ctx) -> Result<()> {
    let chain = &ctx.chain;
    let slot = chain.slot().await?;
    let now = Utc::now().timestamp();
    for vc in &ctx.cfg.vaults {
        if let Err(e) = vault_pass(ctx, vc, slot, now).await {
            tracing::warn!("{}: settle pass aborted: {e:#}", vc.symbol);
        }
    }
    Ok(())
}

async fn vault_pass(ctx: &Ctx, vc: &VaultCfg, slot: u64, now: i64) -> Result<()> {
    let chain = &ctx.chain;
    let sym = &vc.symbol;
    let vault = chain.pdas().vault(&vc.mint);
    let mut v = chain.vault(&vc.mint).await?;

    let due = now >= v.epoch_opened_ts.saturating_add(v.params.epoch_len_secs as i64);
    if v.pending_exit_shares == 0 || !due {
        tracing::info!("{sym}: settle: nothing due (pending={} due={due})", v.pending_exit_shares);
        return Ok(());
    }
    let mut actions: Vec<String> = Vec::new();

    // NAV must be fresh for settle_epoch.
    let nav_age = slot.saturating_sub(v.nav_slot);
    if v.params.max_nav_age_slots > 0 && nav_age > v.params.max_nav_age_slots / 3 {
        let (price, supplies) = {
            let ven = ctx.venues.lock().await;
            (ven.price(sym), ven.supplies_values())
        };
        if price.is_some() || !supplies {
            if chain.try_send(&format!("{sym} refresh_nav (settle)"), chain.ix.refresh_nav(&vault, &vc.oracle, price)).await {
                actions.push("refresh_nav".into());
            }
        } else {
            actions.push("refresh_nav skipped (no price)".into());
        }
    }

    let fraction = exit_fraction_bps(v.pending_exit_shares, v.total_shares);
    match v.state()? {
        VaultState::Unwinding => {
            let ok = finish_unwind(ctx, sym, &vault, v.step).await;
            actions.push(format!("resume unwind from step {} → {}", v.step, if ok { "Parked" } else { "failed" }));
        }
        VaultState::Basis if fraction < 10_000 => {
            let ok = chain.try_send(&format!("{sym} unwind_partial({fraction} bps, exit_demand)"), chain.ix.unwind_partial(&vault, fraction, REASON_EXIT_DEMAND)).await;
            actions.push(format!("unwind_partial {fraction} bps {}", if ok { "ok" } else { "failed" }));
        }
        VaultState::Basis => {
            let ok = chain.try_send(&format!("{sym} unwind_start(exit_demand)"), chain.ix.unwind_start(&vault, REASON_EXIT_DEMAND)).await
                && finish_unwind(ctx, sym, &vault, 0).await;
            actions.push(format!("full unwind {}", if ok { "→ Parked" } else { "failed" }));
        }
        VaultState::Parked | VaultState::Idle | VaultState::Winding => {}
    }

    // Re-read; a full unwind lands in Parked, which then repays.
    v = chain.vault(&vc.mint).await?;
    match v.state()? {
        VaultState::Parked => {
            let ok = chain.try_send(&format!("{sym} repay (settle)"), chain.ix.repay(&vault)).await;
            actions.push(format!("repay from Parked {}", if ok { "ok" } else { "failed" }));
        }
        VaultState::Idle if v.debt_usdc > 0 => {
            let ok = chain.try_send(&format!("{sym} repay residual {} (settle)", v.debt_usdc), chain.ix.repay(&vault)).await;
            actions.push(format!("repay residual debt {} {}", v.debt_usdc, if ok { "ok" } else { "failed" }));
        }
        _ => {}
    }

    let id = v.epoch_id;
    let closed = chain.try_send(&format!("{sym} close_epoch({id})"), chain.ix.close_epoch(&vault, id)).await;
    actions.push(format!("close_epoch({id}) {}", if closed { "ok" } else { "failed" }));
    if closed {
        let settled = chain
            .try_send(&format!("{sym} settle_epoch({id})"), chain.ix.settle_epoch(&vault, id, &vc.mint, &vc.stock_token_program))
            .await;
        actions.push(format!("settle_epoch({id}) {}", if settled { "ok" } else { "failed" }));
    }

    tracing::info!("{sym}: settle: pending={} of {} shares ({fraction} bps): {}", v.pending_exit_shares, v.total_shares, actions.join("; "));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_fraction() {
        assert_eq!(exit_fraction_bps(0, 0), 10_000);
        assert_eq!(exit_fraction_bps(5, 0), 10_000);
        assert_eq!(exit_fraction_bps(100, 100), 10_000);
        assert_eq!(exit_fraction_bps(150, 100), 10_000);
        assert_eq!(exit_fraction_bps(25, 100), 2_500);
        assert_eq!(exit_fraction_bps(1, 3), 3_333);
        assert_eq!(exit_fraction_bps(u64::MAX / 2, u64::MAX), 4_999);
    }
}
