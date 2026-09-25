//! Strategy engine: park/repay, wind (Parked → Basis), unwind (Basis → Parked),
//! size-up and rebalancing (spec §6). Every venue leg goes through `venues::*`,
//! which receive the keeper-supplied venue accounts and parameters via
//! `remaining_accounts` and the trailing `venue_data` argument (see `venues`).

use crate::errors::CarreraError;
use crate::events::Rebalanced;
use crate::rule::Decision;
use crate::state::{UnwindReason, VaultState};
use crate::venues::{self, jupiter, kamino, phoenix, VenueCtx};
use anchor_lang::prelude::*;

use super::{
    check_hedge, check_ltv, check_margin, depositor_qty, evaluate_rule, ltv_bps, margin_bps, mul_bps,
    recompute_nav, require_keeper, require_keeper_or_guardian, require_market_open, require_not_paused,
    require_state, require_step, set_state, stock_value, vault_key, write_off_dust, KeeperVault,
    DEBT_DUST_USDC,
};

pub const REBALANCE_TO_KAMINO: u8 = 0;
pub const REBALANCE_TO_PHOENIX: u8 = 1;
pub const REBALANCE_FROM_PARKED: u8 = 2;

fn add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b).ok_or_else(|| error!(CarreraError::MathOverflow))
}
fn sub(a: u64, b: u64) -> Result<u64> {
    a.checked_sub(b).ok_or_else(|| error!(CarreraError::MathOverflow))
}

/// Build the venue context for a keeper crank from its remaining accounts.
fn vc<'a, 'info>(
    remaining: &'a [AccountInfo<'info>],
    v: &Account<'info, crate::state::OverlayVault>,
    venue_data: &[u8],
) -> Result<VenueCtx<'a, 'info>> {
    VenueCtx::new(remaining, v.to_account_info(), v.xstock_mint, v.bump, venue_data)
}

/// USDC sitting in the vault's `usdc_buffer` (0 when the Kamino block is absent, i.e. mock builds).
fn buffer_usdc(vc: &VenueCtx) -> u64 {
    vc.kamino().ok().and_then(|k| venues::token_amount(&k[21]).ok()).unwrap_or(0)
}

/// USDC the keeper keeps in `usdc_buffer` for Kamino's repay-all rounding; commits leave it there.
pub const BUFFER_CUSHION_USDC: u64 = 50_000;

/// Supply the transit USDC back to Kamino: what the buffer holds beyond the cushion, capped by the
/// parked accounting (the rest of `parked_usdc` is already supplied).
fn supply_transit(vc: &VenueCtx, v: &crate::state::OverlayVault) -> Result<()> {
    if venues::MOCK {
        return kamino::supply_usdc(vc, v.parked_usdc);
    }
    let transit = buffer_usdc(vc).saturating_sub(BUFFER_CUSHION_USDC).min(v.parked_usdc);
    kamino::supply_usdc(vc, transit)
}

/// Target primary loan D = L × value of depositor stock.
fn target_primary_debt(v: &crate::state::OverlayVault) -> Result<u64> {
    let dep_value = stock_value(v, depositor_qty(v))?;
    mul_bps(dep_value, v.params.ltv_bps)
}

/// Borrow the primary loan up to target. Returns the increment borrowed (may be 0).
fn borrow_primary_to_target(vc: &VenueCtx, v: &mut crate::state::OverlayVault) -> Result<u64> {
    let target = target_primary_debt(v)?;
    let inc = target.saturating_sub(v.debt_usdc);
    if inc > 0 {
        kamino::borrow_usdc(vc, inc)?;
        v.debt_usdc = add(v.debt_usdc, inc)?;
    }
    Ok(inc)
}

/// Repay `amount` of the loans, secondary first. Returns what was actually repaid.
fn repay_debt(vc: &VenueCtx, v: &mut crate::state::OverlayVault, amount: u64) -> Result<u64> {
    let amount = amount.min(v.total_debt());
    if amount > 0 {
        kamino::repay_usdc(vc, amount)?;
        let from_b = amount.min(v.debt_b_usdc);
        v.debt_b_usdc = sub(v.debt_b_usdc, from_b)?;
        v.debt_usdc = sub(v.debt_usdc, sub(amount, from_b)?)?;
    }
    Ok(amount)
}

/// Clear the loans entirely. Non-mock builds let Kamino settle the accrued debt
/// (`repay_usdc_all`), which needs the buffer to cover it; Kamino refuses to leave a
/// sub-minimum residual, so a partial repay is not an option here.
fn repay_all(vc: &VenueCtx, v: &mut crate::state::OverlayVault) -> Result<()> {
    if venues::MOCK {
        let d = v.total_debt();
        repay_debt(vc, v, d)?;
    } else {
        kamino::repay_usdc_all(vc)?;
        v.debt_usdc = 0;
        v.debt_b_usdc = 0;
    }
    Ok(())
}

// ---------------------------------------------------------------- park / repay

/// Idle → Parked: borrow D and supply it on Kamino.
pub fn park<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, venue_data: Vec<u8>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    require_not_paused(&ctx.accounts.registry)?;
    let vc = vc(ctx.remaining_accounts, &ctx.accounts.vault, &venue_data)?;
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::Idle)?;
    let d = evaluate_rule(&ctx.accounts.registry, v)?;
    require!(d == Decision::ToParked, CarreraError::RuleNotSatisfied);
    let inc = borrow_primary_to_target(&vc, v)?;
    if inc > 0 {
        kamino::supply_usdc(&vc, inc)?;
        v.parked_usdc = add(v.parked_usdc, inc)?;
    }
    check_ltv(v)?;
    set_state(v, VaultState::Parked, 0);
    recompute_nav(v)?;
    Ok(())
}

/// Parked or Idle → Idle: repay the loans from `usdc_buffer` first, then from
/// supplied USDC. Guard, pause, emergency (guardian) or wind-down only when
/// Parked; from Idle it only clears residual debt. Debt below `DEBT_DUST_USDC`
/// that cannot be covered is written off (spec dust tolerance).
pub fn repay<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, venue_data: Vec<u8>) -> Result<()> {
    let registry = &ctx.accounts.registry;
    let signer = ctx.accounts.keeper.key();
    require_keeper_or_guardian(registry, &signer)?;
    let vc = vc(ctx.remaining_accounts, &ctx.accounts.vault, &venue_data)?;
    let v = &mut ctx.accounts.vault;
    let st = v.vault_state();
    require!(st == VaultState::Parked || st == VaultState::Idle, CarreraError::WrongState);
    let is_guardian = registry.guardian == signer;
    if st == VaultState::Parked && !is_guardian && !registry.paused {
        let d = evaluate_rule(registry, v)?;
        require!(d == Decision::ToIdle, CarreraError::RuleNotSatisfied);
    }
    // 1. bring the supplied USDC back into the buffer
    let need = v.total_debt();
    if need > 0 && v.parked_usdc > 0 {
        let take = need.min(v.parked_usdc);
        kamino::withdraw_supplied_usdc(&vc, take)?;
        v.parked_usdc = sub(v.parked_usdc, take)?;
    }
    // 2. repay: the whole loan when the buffer (plus the dust tolerance) covers it,
    //    otherwise as much as the buffer holds.
    let have = if vc.kamino().is_ok() { buffer_usdc(&vc) } else { need };
    if have.saturating_add(DEBT_DUST_USDC) >= v.total_debt() {
        repay_all(&vc, v)?;
    } else {
        repay_debt(&vc, v, have)?;
    }
    write_off_dust(v);
    set_state(v, VaultState::Idle, 0);
    recompute_nav(v)?;
    Ok(())
}

/// Deposit any xStock sitting in `stock_custody` into the Kamino obligation as
/// collateral (user deposits land in custody; the keeper batches them here).
/// No-op in mock builds.
pub fn sync_collateral<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, venue_data: Vec<u8>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    if venues::MOCK {
        return Ok(());
    }
    let vc = vc(ctx.remaining_accounts, &ctx.accounts.vault, &venue_data)?;
    let k = vc.kamino()?;
    let held = venues::token_amount(&k[20])?;
    // Idempotent: nothing in custody means everything is already in the obligation.
    if held == 0 {
        return Ok(());
    }
    kamino::deposit_collateral(&vc, held)?;
    Ok(())
}

// ---------------------------------------------------------------- wind

/// Parked (or Idle, borrowing D first) → Winding(0). Requires the rule to say ToBasis.
pub fn wind_start<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, venue_data: Vec<u8>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    require_not_paused(&ctx.accounts.registry)?;
    let vc = vc(ctx.remaining_accounts, &ctx.accounts.vault, &venue_data)?;
    let v = &mut ctx.accounts.vault;
    let st = v.vault_state();
    require!(st == VaultState::Parked || st == VaultState::Idle, CarreraError::WrongState);
    require_market_open(v)?;
    let d = evaluate_rule(&ctx.accounts.registry, v)?;
    require!(d == Decision::ToBasis, CarreraError::RuleNotSatisfied);
    if st == VaultState::Idle {
        // Take the primary loan; it sits as transit USDC (in `usdc_buffer`) until step 1 deploys it.
        let inc = borrow_primary_to_target(&vc, v)?;
        v.parked_usdc = add(v.parked_usdc, inc)?;
    }
    require!(v.parked_usdc > 0, CarreraError::RuleNotSatisfied);
    check_ltv(v)?;
    set_state(v, VaultState::Winding, 0);
    Ok(())
}

/// USDC to deploy into Basis from the parked balance, capped by `basis_cap_usdc`.
fn basis_deploy_amount(v: &crate::state::OverlayVault) -> u64 {
    let cap = v.params.basis_cap_usdc;
    if cap == 0 {
        v.parked_usdc
    } else {
        let room = cap.saturating_sub(stock_value(v, v.basis_spot_qty).unwrap_or(u64::MAX));
        v.parked_usdc.min(room)
    }
}

pub fn wind_step<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, n: u8, venue_data: Vec<u8>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let vc = vc(ctx.remaining_accounts, &ctx.accounts.vault, &venue_data)?;
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::Winding)?;
    deploy_step(&vc, v, n)
}

/// One deployment step, shared by `wind_step` (Winding) and `size_up_step` (SizingUp): the transit
/// USDC in `parked_usdc` becomes spot on Kamino (1), D_b on Phoenix (2) and the short (3).
fn deploy_step(vc: &VenueCtx, v: &mut crate::state::OverlayVault, n: u8) -> Result<()> {
    require!(n >= 1 && n <= 3, CarreraError::InvalidArgument);
    require_step(v, n - 1)?;
    require_market_open(v)?;
    match n {
        1 => {
            // Supplied USDC (unless it is still transit from an Idle start) → Jupiter USDC→xStock → Kamino collateral.
            let usdc = basis_deploy_amount(v);
            require!(usdc > 0, CarreraError::InvalidArgument);
            let in_buffer = buffer_usdc(vc);
            if usdc > in_buffer {
                kamino::withdraw_supplied_usdc(vc, usdc - in_buffer)?;
            }
            let out = jupiter::swap_usdc_to_stock(vc, v, usdc)?;
            let min_out = mul_bps(
                crate::nav::stock_qty_from_usdc(usdc, v.price_e6, v.stock_decimals).ok_or(CarreraError::MathOverflow)?,
                10_000 - v.params.max_swap_slippage_bps,
            )?;
            require!(out >= min_out, CarreraError::SlippageExceeded);
            kamino::deposit_collateral(vc, out)?;
            v.parked_usdc = sub(v.parked_usdc, usdc)?;
            v.collateral_qty = add(v.collateral_qty, out)?;
            v.basis_spot_qty = add(v.basis_spot_qty, out)?;
            check_ltv(v)?;
        }
        2 => {
            // Borrow D_b = L × basis notional → Phoenix subaccount collateral.
            let notional = stock_value(v, v.basis_spot_qty)?;
            let target = mul_bps(notional, v.params.ltv_bps)?;
            let d_b = target.saturating_sub(v.debt_b_usdc);
            if d_b > 0 {
                kamino::borrow_usdc(vc, d_b)?;
                phoenix::deposit_collateral(vc, d_b)?;
                v.debt_b_usdc = add(v.debt_b_usdc, d_b)?;
                v.phoenix_equity_usdc = add(v.phoenix_equity_usdc, d_b)?;
            }
            check_ltv(v)?;
        }
        3 => {
            // Short the basis spot quantity on Phoenix.
            let qty = sub(v.basis_spot_qty, v.phoenix_short_qty.min(v.basis_spot_qty))?;
            if qty > 0 {
                let filled = phoenix::open_short(vc, v, qty)?;
                let min_fill = mul_bps(qty, 10_000 - v.params.max_perp_slippage_bps)?;
                require!(filled >= min_fill, CarreraError::SlippageExceeded);
                v.phoenix_short_qty = add(v.phoenix_short_qty, filled)?;
            }
        }
        _ => unreachable!(),
    }
    v.step = n;
    recompute_nav(v)?;
    Ok(())
}

pub fn wind_commit<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, venue_data: Vec<u8>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let vc = vc(ctx.remaining_accounts, &ctx.accounts.vault, &venue_data)?;
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::Winding)?;
    commit_basis(&vc, v)
}

/// Step 3 done → Basis, after the hedge, margin and LTV checks (shared by wind and size-up).
fn commit_basis(vc: &VenueCtx, v: &mut crate::state::OverlayVault) -> Result<()> {
    require_step(v, 3)?;
    v.phoenix_equity_usdc = phoenix::read_equity(vc, v)?;
    check_hedge(v)?;
    check_margin(v)?;
    check_ltv(v)?;
    set_state(v, VaultState::Basis, 0);
    recompute_nav(v)?;
    Ok(())
}

/// Winding(n) → Unwinding(0). The unwind steps tolerate zero-size legs, so
/// running unwind 1..3 + commit rolls back whatever wind had completed.
pub fn wind_abort(ctx: Context<KeeperVault>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::Winding)?;
    set_state(v, VaultState::Unwinding, 0);
    Ok(())
}

// ---------------------------------------------------------------- unwind

pub fn unwind_start(ctx: Context<KeeperVault>, reason: u8) -> Result<()> {
    let registry = &ctx.accounts.registry;
    let signer = ctx.accounts.keeper.key();
    let reason = UnwindReason::from_u8(reason).ok_or(CarreraError::InvalidArgument)?;
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::Basis)?;
    match reason {
        UnwindReason::Rule => {
            require_keeper(registry, &signer)?;
            require_market_open(v)?;
            let d = evaluate_rule(registry, v)?;
            require!(d == Decision::ToParked || d == Decision::ToIdle, CarreraError::RuleNotSatisfied);
        }
        UnwindReason::ExitDemand => {
            require_keeper(registry, &signer)?;
            require_market_open(v)?;
            require!(v.pending_exit_shares > 0, CarreraError::RuleNotSatisfied);
        }
        UnwindReason::Emergency => {
            require_keeper_or_guardian(registry, &signer)?;
            if registry.guardian != signer {
                let unhealthy = ltv_bps(v)? > v.params.emergency_ltv_bps
                    || margin_bps(v)? < v.params.min_margin_bps;
                require!(unhealthy, CarreraError::RuleNotSatisfied);
            }
        }
    }
    set_state(v, VaultState::Unwinding, 0);
    Ok(())
}

pub fn unwind_step<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, n: u8, venue_data: Vec<u8>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let vc = vc(ctx.remaining_accounts, &ctx.accounts.vault, &venue_data)?;
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::Unwinding)?;
    require!(n >= 1 && n <= 3, CarreraError::InvalidArgument);
    require_step(v, n - 1)?;
    match n {
        1 => {
            // Close the short (reduce-only).
            if v.phoenix_short_qty > 0 {
                let (filled, pnl) = phoenix::close_short(&vc, v, v.phoenix_short_qty)?;
                v.phoenix_short_qty = sub(v.phoenix_short_qty, filled)?;
                v.phoenix_equity_usdc = apply_pnl(v.phoenix_equity_usdc, pnl)?;
            }
        }
        2 => {
            // Withdraw Phoenix collateral → repay min(recovered, D_b); surplus → transit (parked_usdc).
            let equity = phoenix::read_equity(&vc, v)?;
            if equity > 0 {
                phoenix::withdraw_collateral(&vc, equity)?;
            }
            let repay_amt = equity.min(v.debt_b_usdc);
            if repay_amt > 0 {
                kamino::repay_usdc(&vc, repay_amt)?;
            }
            v.debt_b_usdc = sub(v.debt_b_usdc, repay_amt)?;
            v.parked_usdc = add(v.parked_usdc, sub(equity, repay_amt)?)?;
            v.phoenix_equity_usdc = 0;
        }
        3 => {
            // Withdraw basis spot → Jupiter xStock→USDC → transit.
            let qty = v.basis_spot_qty;
            if qty > 0 {
                kamino::withdraw_collateral(&vc, qty)?;
                let usdc = jupiter::swap_stock_to_usdc(&vc, v, qty)?;
                let min_out = mul_bps(stock_value(v, qty)?, 10_000 - v.params.max_swap_slippage_bps)?;
                require!(usdc >= min_out, CarreraError::SlippageExceeded);
                v.collateral_qty = sub(v.collateral_qty, qty)?;
                v.basis_spot_qty = 0;
                v.parked_usdc = add(v.parked_usdc, usdc)?;
            }
        }
        _ => unreachable!(),
    }
    v.step = n;
    recompute_nav(v)?;
    Ok(())
}

/// Unwinding(3) → Parked: clear residual D_b from the recovered USDC (shortfall
/// below dust is written off, above dust folds into the primary loan), then
/// supply what is left on Kamino.
pub fn unwind_commit<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, venue_data: Vec<u8>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let vc = vc(ctx.remaining_accounts, &ctx.accounts.vault, &venue_data)?;
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::Unwinding)?;
    require_step(v, 3)?;
    if v.debt_b_usdc > 0 {
        let repay_amt = v.debt_b_usdc.min(v.parked_usdc);
        if repay_amt > 0 {
            kamino::repay_usdc(&vc, repay_amt)?;
            v.parked_usdc = sub(v.parked_usdc, repay_amt)?;
        }
        let residual = sub(v.debt_b_usdc, repay_amt)?;
        v.debt_b_usdc = 0;
        if residual >= DEBT_DUST_USDC {
            v.debt_usdc = add(v.debt_usdc, residual)?;
        }
    }
    if v.parked_usdc > 0 {
        supply_transit(&vc, v)?;
    }
    write_off_dust(v);
    check_ltv(v)?;
    set_state(v, VaultState::Parked, 0);
    recompute_nav(v)?;
    Ok(())
}

/// Basis → PartialUnwinding(0): release `fraction_bps` of the basis position over three steps
/// (`unwind_partial_step`) and `unwind_partial_commit`. The fraction is repeated on every step (the
/// vault has no spare field for it); the keeper derives it from the pending exits each time and the
/// commit's hedge check catches a mismatch.
pub fn unwind_partial_start(ctx: Context<KeeperVault>, fraction_bps: u32, reason: u8) -> Result<()> {
    let registry = &ctx.accounts.registry;
    let signer = ctx.accounts.keeper.key();
    let reason = UnwindReason::from_u8(reason).ok_or(CarreraError::InvalidArgument)?;
    require!(fraction_bps > 0 && fraction_bps <= 10_000, CarreraError::InvalidArgument);
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::Basis)?;
    match reason {
        UnwindReason::Emergency => require_keeper_or_guardian(registry, &signer)?,
        _ => {
            require_keeper(registry, &signer)?;
            require_market_open(v)?;
        }
    }
    set_state(v, VaultState::PartialUnwinding, 0);
    Ok(())
}

/// 1 (Phoenix): close `fraction` of the short. 2 (Phoenix + Kamino): withdraw `fraction` of the
/// live equity, repay up to `fraction` of D_b, the rest becomes transit. 3 (Kamino + Jupiter):
/// withdraw and sell `fraction` of the basis spot; the proceeds repay `fraction` of the primary loan
/// (the leaving depositors' share of D, so their stock can settle out) and the rest becomes transit.
pub fn unwind_partial_step<'info>(
    ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>,
    n: u8,
    fraction_bps: u32,
    venue_data: Vec<u8>,
) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    require!(fraction_bps > 0 && fraction_bps <= 10_000, CarreraError::InvalidArgument);
    let vc = vc(ctx.remaining_accounts, &ctx.accounts.vault, &venue_data)?;
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::PartialUnwinding)?;
    require!(n >= 1 && n <= 3, CarreraError::InvalidArgument);
    require_step(v, n - 1)?;
    match n {
        1 => {
            let short_qty = mul_bps(v.phoenix_short_qty, fraction_bps)?;
            if short_qty > 0 {
                let (filled, pnl) = phoenix::close_short(&vc, v, short_qty)?;
                v.phoenix_short_qty = sub(v.phoenix_short_qty, filled)?;
                v.phoenix_equity_usdc = apply_pnl(v.phoenix_equity_usdc, pnl)?;
            }
        }
        2 => {
            let equity = phoenix::read_equity(&vc, v)?;
            let withdraw = mul_bps(equity, fraction_bps)?;
            if withdraw > 0 {
                phoenix::withdraw_collateral(&vc, withdraw)?;
            }
            v.phoenix_equity_usdc = sub(equity, withdraw)?;
            let repay_amt = withdraw.min(mul_bps(v.debt_b_usdc, fraction_bps)?).min(v.debt_b_usdc);
            if repay_amt > 0 {
                kamino::repay_usdc(&vc, repay_amt)?;
                v.debt_b_usdc = sub(v.debt_b_usdc, repay_amt)?;
            }
            v.parked_usdc = add(v.parked_usdc, sub(withdraw, repay_amt)?)?;
        }
        3 => {
            let qty = mul_bps(v.basis_spot_qty, fraction_bps)?;
            if qty > 0 {
                kamino::withdraw_collateral(&vc, qty)?;
                let usdc = jupiter::swap_stock_to_usdc(&vc, v, qty)?;
                let min_out = mul_bps(stock_value(v, qty)?, 10_000 - v.params.max_swap_slippage_bps)?;
                require!(usdc >= min_out, CarreraError::SlippageExceeded);
                v.collateral_qty = sub(v.collateral_qty, qty)?;
                v.basis_spot_qty = sub(v.basis_spot_qty, qty)?;
                let repay_d = mul_bps(v.debt_usdc, fraction_bps)?.min(usdc);
                if repay_d > 0 {
                    kamino::repay_usdc(&vc, repay_d)?;
                    v.debt_usdc = sub(v.debt_usdc, repay_d)?;
                }
                v.parked_usdc = add(v.parked_usdc, sub(usdc, repay_d)?)?;
            }
        }
        _ => unreachable!(),
    }
    v.step = n;
    recompute_nav(v)?;
    Ok(())
}

/// PartialUnwinding(3) → Basis: supply the transit USDC on Kamino, then the hedge, LTV and margin checks.
pub fn unwind_partial_commit<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, venue_data: Vec<u8>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let vc = vc(ctx.remaining_accounts, &ctx.accounts.vault, &venue_data)?;
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::PartialUnwinding)?;
    require_step(v, 3)?;
    if v.parked_usdc > 0 {
        supply_transit(&vc, v)?;
    }
    check_hedge(v)?;
    // Releasing spot while D stays at L × deposits leaves the LTV at L (plus rounding and any
    // price move since entry), so the release may end anywhere inside the rebalance band; the
    // fast loop's `rebalance_to_kamino` brings it back to L.
    require!(
        ltv_bps(v)? <= v.params.ltv_bps.saturating_add(v.params.rebalance_ltv_band_bps),
        CarreraError::LtvTooHigh
    );
    check_margin(v)?;
    set_state(v, VaultState::Basis, 0);
    recompute_nav(v)?;
    Ok(())
}

/// PartialUnwinding(n) → Unwinding(0): give up on the partial release and unwind everything
/// (the full unwind steps tolerate the legs a partial step already reduced).
pub fn unwind_partial_abort(ctx: Context<KeeperVault>) -> Result<()> {
    require_keeper_or_guardian(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::PartialUnwinding)?;
    set_state(v, VaultState::Unwinding, 0);
    Ok(())
}

fn apply_pnl(equity: u64, pnl: i64) -> Result<u64> {
    let e = (equity as i128) + (pnl as i128);
    u64::try_from(e.max(0)).map_err(|_| error!(CarreraError::MathOverflow))
}

// ---------------------------------------------------------------- size up

/// Borrow up to target after deposits. Parked: the increment is supplied on Kamino and the vault
/// stays Parked (one Kamino step). Basis: the increment becomes transit and the vault goes to
/// SizingUp(0); `size_up_step(1..3)` deploy it exactly like `wind_step` and `size_up_commit` returns
/// to Basis. Keeper batches, never per deposit.
pub fn size_up_start<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, venue_data: Vec<u8>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    require_not_paused(&ctx.accounts.registry)?;
    let vc = vc(ctx.remaining_accounts, &ctx.accounts.vault, &venue_data)?;
    let v = &mut ctx.accounts.vault;
    let st = v.vault_state();
    require!(st == VaultState::Parked || st == VaultState::Basis, CarreraError::WrongState);
    require_market_open(v)?;
    let ltv_now = ltv_bps(v)?;
    require!(
        ltv_now < v.params.ltv_bps.saturating_sub(v.params.size_band_bps),
        CarreraError::RuleNotSatisfied
    );
    let inc = borrow_primary_to_target(&vc, v)?;
    require!(inc > 0, CarreraError::RuleNotSatisfied);
    v.parked_usdc = add(v.parked_usdc, inc)?;
    match st {
        VaultState::Parked => kamino::supply_usdc(&vc, inc)?,
        VaultState::Basis => set_state(v, VaultState::SizingUp, 0),
        _ => unreachable!(),
    }
    check_ltv(v)?;
    recompute_nav(v)?;
    Ok(())
}

pub fn size_up_step<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, n: u8, venue_data: Vec<u8>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let vc = vc(ctx.remaining_accounts, &ctx.accounts.vault, &venue_data)?;
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::SizingUp)?;
    deploy_step(&vc, v, n)
}

pub fn size_up_commit<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, venue_data: Vec<u8>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let vc = vc(ctx.remaining_accounts, &ctx.accounts.vault, &venue_data)?;
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::SizingUp)?;
    commit_basis(&vc, v)
}

/// SizingUp(n) → Unwinding(0): like `wind_abort`, the whole position is unwound.
pub fn size_up_abort(ctx: Context<KeeperVault>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::SizingUp)?;
    set_state(v, VaultState::Unwinding, 0);
    Ok(())
}

// ---------------------------------------------------------------- rebalancing

/// Basis, LTV > L + band: move free Phoenix collateral to Kamino and repay.
pub fn rebalance_to_kamino<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, venue_data: Vec<u8>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let vc = vc(ctx.remaining_accounts, &ctx.accounts.vault, &venue_data)?;
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::Basis)?;
    require!(
        ltv_bps(v)? > v.params.ltv_bps + v.params.rebalance_ltv_band_bps,
        CarreraError::RuleNotSatisfied
    );
    v.phoenix_equity_usdc = phoenix::read_equity(&vc, v)?;
    let needed = sub(v.total_debt(), mul_bps(stock_value(v, v.collateral_qty)?, v.params.ltv_bps)?)?;
    let required_margin = mul_bps(stock_value(v, v.phoenix_short_qty)?, v.params.min_margin_bps)?;
    let free = v.phoenix_equity_usdc.saturating_sub(required_margin);
    let amount = needed.min(free);
    require!(amount > 0, CarreraError::MarginTooLow);
    phoenix::withdraw_collateral(&vc, amount)?;
    v.phoenix_equity_usdc = sub(v.phoenix_equity_usdc, amount)?;
    repay_debt(&vc, v, amount)?;
    check_margin(v)?;
    recompute_nav(v)?;
    emit!(Rebalanced { vault: vault_key(v), kind: REBALANCE_TO_KAMINO, amount });
    Ok(())
}

/// Basis, margin < min + band: borrow on Kamino and top up Phoenix.
pub fn rebalance_to_phoenix<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, venue_data: Vec<u8>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let vc = vc(ctx.remaining_accounts, &ctx.accounts.vault, &venue_data)?;
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::Basis)?;
    v.phoenix_equity_usdc = phoenix::read_equity(&vc, v)?;
    let floor = v.params.min_margin_bps + v.params.rebalance_margin_band_bps;
    require!(margin_bps(v)? < floor, CarreraError::RuleNotSatisfied);
    let target = mul_bps(stock_value(v, v.phoenix_short_qty)?, floor)?;
    let amount = sub(target, v.phoenix_equity_usdc)?;
    require!(amount > 0, CarreraError::RuleNotSatisfied);
    kamino::borrow_usdc(&vc, amount)?;
    phoenix::deposit_collateral(&vc, amount)?;
    v.debt_b_usdc = add(v.debt_b_usdc, amount)?;
    v.phoenix_equity_usdc = add(v.phoenix_equity_usdc, amount)?;
    check_ltv(v)?;
    recompute_nav(v)?;
    emit!(Rebalanced { vault: vault_key(v), kind: REBALANCE_TO_PHOENIX, amount });
    Ok(())
}

/// Parked, LTV > L + band (or exits pending that need headroom, spec §8): withdraw supplied USDC and repay.
pub fn rebalance_from_parked<'info>(
    ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>,
    amount: u64,
    venue_data: Vec<u8>,
) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let vc = vc(ctx.remaining_accounts, &ctx.accounts.vault, &venue_data)?;
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::Parked)?;
    require!(
        v.pending_exit_shares > 0 || ltv_bps(v)? > v.params.ltv_bps + v.params.rebalance_ltv_band_bps,
        CarreraError::RuleNotSatisfied
    );
    let amount = amount.min(v.parked_usdc).min(v.debt_usdc);
    require!(amount > 0, CarreraError::InvalidArgument);
    kamino::withdraw_supplied_usdc(&vc, amount)?;
    let have = if vc.kamino().is_ok() { buffer_usdc(&vc) } else { amount };
    let pay = amount.min(have);
    kamino::repay_usdc(&vc, pay)?;
    v.parked_usdc = sub(v.parked_usdc, amount)?;
    v.debt_usdc = sub(v.debt_usdc, pay)?;
    write_off_dust(v);
    recompute_nav(v)?;
    emit!(Rebalanced { vault: vault_key(v), kind: REBALANCE_FROM_PARKED, amount });
    Ok(())
}
