//! Settlement pass, every `settle_interval_secs` (default 5 min). Frees the
//! liability of due exit epochs (unwind / repay as the state requires), then
//! closes and settles them, so exits settle without waiting for the hourly pass.
//!
//! Per vault, every pass:
//!   1. previous epoch (`epoch_id − 1`): if it exists, is closed and not settled,
//!      release and `settle_epoch` it regardless of timing (a close that ran without
//!      a settle must not wait on the next epoch's window);
//!   2. current epoch: when it holds shares and its window has elapsed, release,
//!      `close_epoch`, `settle_epoch`.
//!
//! Release by state: Unwinding → resume `unwind_step` from the current step and
//! `unwind_commit`; Basis → `unwind_partial(fraction, ExitDemand)` when the exit is
//! smaller than the whole position, else `unwind_start(ExitDemand)` + steps + commit;
//! then Parked → `repay`; Idle with residual `debt_usdc` above dust → `repay` (the
//! hotfixed program accepts repay from Idle and ignores dust ≤ 10_000 in settlement).
//! Every send is best-effort; the program is the authority. One log line per vault.

use crate::{
    venue::{self, Swap},
    accounts::{ExitEpoch, OverlayVault, VaultState},
    config::VaultCfg,
    hourly::finish_unwind,
    ix::REASON_EXIT_DEMAND,
    Ctx,
};
use anyhow::Result;
use chrono::Utc;
use solana_sdk::pubkey::Pubkey;

/// Debt the hotfixed program ignores at settlement; not worth a `repay`.
pub const DEBT_DUST_USDC: u64 = 10_000;

/// Share of the position leaving, in bps, capped at 10 000.
pub fn exit_fraction_bps(exiting_shares: u64, total_shares: u64) -> u32 {
    if total_shares == 0 || exiting_shares >= total_shares {
        return 10_000;
    }
    (exiting_shares as u128 * 10_000 / total_shares as u128) as u32
}

/// The previous epoch's id when it still needs settling: it exists, was closed, and
/// is not settled. `prev` is the decoded `ExitEpoch` for `epoch_id − 1`, if any.
pub fn previous_epoch_to_settle(epoch_id: u64, prev: Option<&ExitEpoch>) -> Option<u64> {
    if epoch_id == 0 {
        return None;
    }
    match prev {
        Some(e) if e.closed && !e.settled => Some(epoch_id - 1),
        _ => None,
    }
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
    let mut actions: Vec<String> = Vec::new();

    // 1. Previous epoch closed but never settled.
    let prev = if v.epoch_id > 0 { chain.epoch(&vault, v.epoch_id - 1).await? } else { None };
    if let Some(id) = previous_epoch_to_settle(v.epoch_id, prev.as_ref()) {
        let shares = prev.as_ref().map(|e| e.shares_total).unwrap_or(0);
        let fraction = exit_fraction_bps(shares, v.total_shares.saturating_add(shares));
        actions.push(format!("prev epoch {id} closed-unsettled ({shares} shares, {fraction} bps)"));
        release(ctx, vc, &vault, &mut v, slot, fraction, &mut actions).await?;
        let ok = venue::send_crank(ctx, vc, &v, Swap::None, venue::NEED_KP, &format!("{sym} settle_epoch({id}) prev"), |a| chain.ix.settle_epoch(&vault, id, &vc.mint, &vc.stock_token_program, a)).await;
        actions.push(format!("settle_epoch({id}) {}", if ok { "ok" } else { "failed" }));
    }

    // 2. Current epoch.
    let due = now >= v.epoch_opened_ts.saturating_add(v.params.epoch_len_secs as i64);
    if v.pending_exit_shares > 0 && due {
        let id = v.epoch_id;
        let fraction = exit_fraction_bps(v.pending_exit_shares, v.total_shares);
        actions.push(format!("epoch {id} due ({} of {} shares, {fraction} bps)", v.pending_exit_shares, v.total_shares));
        release(ctx, vc, &vault, &mut v, slot, fraction, &mut actions).await?;
        let closed = chain.try_send(&format!("{sym} close_epoch({id})"), chain.ix.close_epoch(&vault, id)).await;
        actions.push(format!("close_epoch({id}) {}", if closed { "ok" } else { "failed" }));
        if closed {
            let settled = venue::send_crank_fresh(ctx, vc, |_| Swap::None, venue::NEED_KP, &format!("{sym} settle_epoch({id})"), |a| chain.ix.settle_epoch(&vault, id, &vc.mint, &vc.stock_token_program, a)).await;
            actions.push(format!("settle_epoch({id}) {}", if settled { "ok" } else { "failed" }));
        }
    } else if actions.is_empty() {
        actions.push(format!("nothing due (pending={} due={due})", v.pending_exit_shares));
    }

    tracing::info!("{sym}: settle: {}", actions.join("; "));
    Ok(())
}

/// Free enough of the loan for a settlement of `fraction` bps of the position, then
/// re-read the vault into `v`.
async fn release(ctx: &Ctx, vc: &VaultCfg, vault: &Pubkey, v: &mut OverlayVault, slot: u64, fraction: u32, actions: &mut Vec<String>) -> Result<()> {
    let chain = &ctx.chain;
    let sym = &vc.symbol;

    // NAV must be fresh for settle_epoch.
    let nav_age = slot.saturating_sub(v.nav_slot);
    if v.params.max_nav_age_slots > 0 && nav_age > v.params.max_nav_age_slots / 3 {
        let (price, supplies) = {
            let ven = ctx.venues.lock().await;
            (ven.price(sym), ven.supplies_values())
        };
        if ctx.cfg.program_build == crate::config::ProgramBuild::Real || price.is_some() || !supplies {
            if crate::hourly::refresh_nav(ctx, vc, price, "refresh_nav (settle)").await {
                actions.push("refresh_nav".into());
            }
        } else {
            actions.push("refresh_nav skipped (no price)".into());
        }
    }

    match v.state()? {
        VaultState::Unwinding => {
            let ok = finish_unwind(ctx, vc, v.step).await;
            actions.push(format!("resume unwind from step {} {}", v.step, if ok { "→ Parked" } else { "failed" }));
        }
        VaultState::Basis if fraction < 10_000 => {
            let swap = Swap::StockToUsdc(venue::partial_sell_qty(v, fraction));
            let ok = venue::send_crank(ctx, vc, v, swap, venue::NEED_KP, &format!("{sym} unwind_partial({fraction} bps, exit_demand)"), |a| chain.ix.unwind_partial(vault, fraction, REASON_EXIT_DEMAND, a)).await;
            actions.push(format!("unwind_partial {fraction} bps {}", if ok { "ok" } else { "failed" }));
        }
        VaultState::Basis => {
            let ok = chain.try_send(&format!("{sym} unwind_start(exit_demand)"), chain.ix.unwind_start(vault, REASON_EXIT_DEMAND)).await
                && finish_unwind(ctx, vc, 0).await;
            actions.push(format!("full unwind {}", if ok { "→ Parked" } else { "failed" }));
        }
        VaultState::Parked | VaultState::Idle | VaultState::Winding => {}
    }

    // Re-read; a full unwind lands in Parked, which then repays.
    *v = chain.vault(&vc.mint).await?;
    match v.state()? {
        VaultState::Parked => {
            let ok = venue::send_crank(ctx, vc, v, Swap::None, venue::NEED_K, &format!("{sym} repay (settle)"), |a| chain.ix.repay(vault, a)).await;
            actions.push(format!("repay from Parked {}", if ok { "ok" } else { "failed" }));
        }
        VaultState::Idle if v.debt_usdc > DEBT_DUST_USDC => {
            let ok = venue::send_crank(ctx, vc, v, Swap::None, venue::NEED_K, &format!("{sym} repay residual {} (settle)", v.debt_usdc), |a| chain.ix.repay(vault, a)).await;
            actions.push(format!("repay residual debt {} {}", v.debt_usdc, if ok { "ok" } else { "failed" }));
        }
        _ => {}
    }
    *v = chain.vault(&vc.mint).await?;
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

    #[test]
    fn previous_epoch_selection() {
        let ep = |closed, settled| ExitEpoch { id: 0, shares_total: 2_959_787, closed, settled, ..Default::default() };
        // No previous epoch at genesis.
        assert_eq!(previous_epoch_to_settle(0, Some(&ep(true, false))), None);
        // The live AAPL case: epoch 0 closed, unsettled, epoch_id already 1.
        assert_eq!(previous_epoch_to_settle(1, Some(&ep(true, false))), Some(0));
        assert_eq!(previous_epoch_to_settle(7, Some(&ep(true, false))), Some(6));
        // Already settled, still open, or account missing → nothing to do.
        assert_eq!(previous_epoch_to_settle(1, Some(&ep(true, true))), None);
        assert_eq!(previous_epoch_to_settle(1, Some(&ep(false, false))), None);
        assert_eq!(previous_epoch_to_settle(1, None), None);
    }
}
