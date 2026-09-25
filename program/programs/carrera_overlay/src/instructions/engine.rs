//! Strategy engine: park/repay, wind (Parked → Basis), unwind (Basis → Parked),
//! size-up and rebalancing (spec §6). Every venue leg goes through `venues::*`.

use crate::errors::CarreraError;
use crate::events::Rebalanced;
use crate::rule::Decision;
use crate::state::{UnwindReason, VaultState};
use crate::venues::{jupiter, kamino, phoenix};
use anchor_lang::prelude::*;

use super::{
    check_hedge, check_ltv, check_margin, depositor_qty, evaluate_rule, ltv_bps, margin_bps, mul_bps,
    recompute_nav, require_keeper, require_keeper_or_guardian, require_market_open, require_not_paused,
    require_state, require_step, set_state, stock_value, vault_key, KeeperVault,
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

/// Target primary loan D = L × value of depositor stock.
fn target_primary_debt(v: &crate::state::OverlayVault) -> Result<u64> {
    let dep_value = stock_value(v, depositor_qty(v))?;
    mul_bps(dep_value, v.params.ltv_bps)
}

/// Borrow the primary loan up to target. Returns the increment borrowed (may be 0).
fn borrow_primary_to_target(v: &mut crate::state::OverlayVault) -> Result<u64> {
    let target = target_primary_debt(v)?;
    let inc = target.saturating_sub(v.debt_usdc);
    if inc > 0 {
        kamino::borrow_usdc(inc)?;
        v.debt_usdc = add(v.debt_usdc, inc)?;
    }
    Ok(inc)
}

// ---------------------------------------------------------------- park / repay

/// Idle → Parked: borrow D and supply it on Kamino.
pub fn park(ctx: Context<KeeperVault>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    require_not_paused(&ctx.accounts.registry)?;
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::Idle)?;
    let d = evaluate_rule(&ctx.accounts.registry, v)?;
    require!(d == Decision::ToParked, CarreraError::RuleNotSatisfied);
    let inc = borrow_primary_to_target(v)?;
    if inc > 0 {
        kamino::supply_usdc(inc)?;
        v.parked_usdc = add(v.parked_usdc, inc)?;
    }
    check_ltv(v)?;
    set_state(v, VaultState::Parked, 0);
    recompute_nav(v)?;
    Ok(())
}

/// Parked → Idle: withdraw supplied USDC and repay. Guard, pause, emergency (guardian) or wind-down only.
pub fn repay(ctx: Context<KeeperVault>) -> Result<()> {
    let registry = &ctx.accounts.registry;
    let signer = ctx.accounts.keeper.key();
    require_keeper_or_guardian(registry, &signer)?;
    let v = &mut ctx.accounts.vault;
    // Parked → Idle by rule/guard, or Idle → Idle to clear residual debt with whatever USDC the vault holds.
    let st = v.vault_state();
    require!(st == VaultState::Parked || st == VaultState::Idle, CarreraError::WrongState);
    let is_guardian = registry.guardian == signer;
    if st == VaultState::Parked && !is_guardian && !registry.paused {
        let d = evaluate_rule(registry, v)?;
        require!(d == Decision::ToIdle, CarreraError::RuleNotSatisfied);
    }
    let amount = v.parked_usdc;
    if amount > 0 {
        kamino::withdraw_supplied_usdc(amount)?;
    }
    let repay_amt = amount.min(v.debt_usdc);
    if repay_amt > 0 {
        kamino::repay_usdc(repay_amt)?;
    }
    v.debt_usdc = sub(v.debt_usdc, repay_amt)?;
    // Any residual (earned interest) stays supplied and counts toward NAV.
    v.parked_usdc = sub(v.parked_usdc, repay_amt)?;
    set_state(v, VaultState::Idle, 0);
    recompute_nav(v)?;
    Ok(())
}

// ---------------------------------------------------------------- wind

/// Parked (or Idle, borrowing D first) → Winding(0). Requires the rule to say ToBasis.
pub fn wind_start(ctx: Context<KeeperVault>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    require_not_paused(&ctx.accounts.registry)?;
    let v = &mut ctx.accounts.vault;
    let st = v.vault_state();
    require!(st == VaultState::Parked || st == VaultState::Idle, CarreraError::WrongState);
    require_market_open(v)?;
    let d = evaluate_rule(&ctx.accounts.registry, v)?;
    require!(d == Decision::ToBasis, CarreraError::RuleNotSatisfied);
    if st == VaultState::Idle {
        // Take the primary loan; it sits as transit USDC until step 1 deploys it.
        let inc = borrow_primary_to_target(v)?;
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

pub fn wind_step(ctx: Context<KeeperVault>, n: u8) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::Winding)?;
    require!(n >= 1 && n <= 3, CarreraError::InvalidArgument);
    require_step(v, n - 1)?;
    require_market_open(v)?;
    match n {
        1 => {
            // Withdraw supplied USDC → Jupiter USDC→xStock → Kamino collateral.
            let usdc = basis_deploy_amount(v);
            require!(usdc > 0, CarreraError::InvalidArgument);
            kamino::withdraw_supplied_usdc(usdc)?;
            let out = jupiter::swap_usdc_to_stock(v, usdc)?;
            let min_out = mul_bps(
                crate::nav::stock_qty_from_usdc(usdc, v.price_e6, v.stock_decimals).ok_or(CarreraError::MathOverflow)?,
                10_000 - v.params.max_swap_slippage_bps,
            )?;
            require!(out >= min_out, CarreraError::SlippageExceeded);
            kamino::deposit_collateral(out)?;
            v.parked_usdc = sub(v.parked_usdc, usdc)?;
            v.collateral_qty = add(v.collateral_qty, out)?;
            v.basis_spot_qty = add(v.basis_spot_qty, out)?;
            check_ltv(v)?;
        }
        2 => {
            // Borrow D_b = L × basis notional → Phoenix subaccount collateral.
            let notional = stock_value(v, v.basis_spot_qty)?;
            let d_b = sub(mul_bps(notional, v.params.ltv_bps)?, v.debt_b_usdc.min(mul_bps(notional, v.params.ltv_bps)?))?;
            if d_b > 0 {
                kamino::borrow_usdc(d_b)?;
                phoenix::deposit_collateral(d_b)?;
                v.debt_b_usdc = add(v.debt_b_usdc, d_b)?;
                v.phoenix_equity_usdc = add(v.phoenix_equity_usdc, d_b)?;
            }
            check_ltv(v)?;
        }
        3 => {
            // Short the basis spot quantity on Phoenix.
            let qty = sub(v.basis_spot_qty, v.phoenix_short_qty.min(v.basis_spot_qty))?;
            if qty > 0 {
                let filled = phoenix::open_short(v, qty)?;
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

pub fn wind_commit(ctx: Context<KeeperVault>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::Winding)?;
    require_step(v, 3)?;
    v.phoenix_equity_usdc = phoenix::read_equity(v)?;
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

pub fn unwind_step(ctx: Context<KeeperVault>, n: u8) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::Unwinding)?;
    require!(n >= 1 && n <= 3, CarreraError::InvalidArgument);
    require_step(v, n - 1)?;
    match n {
        1 => {
            // Close the short (reduce-only).
            if v.phoenix_short_qty > 0 {
                let (filled, pnl) = phoenix::close_short(v, v.phoenix_short_qty)?;
                v.phoenix_short_qty = sub(v.phoenix_short_qty, filled)?;
                v.phoenix_equity_usdc = apply_pnl(v.phoenix_equity_usdc, pnl)?;
            }
        }
        2 => {
            // Withdraw Phoenix collateral → repay D_b; surplus → transit (parked_usdc).
            let equity = phoenix::read_equity(v)?;
            if equity > 0 {
                phoenix::withdraw_collateral(equity)?;
            }
            let repay_amt = equity.min(v.debt_b_usdc);
            if repay_amt > 0 {
                kamino::repay_usdc(repay_amt)?;
            }
            v.debt_b_usdc = sub(v.debt_b_usdc, repay_amt)?;
            v.parked_usdc = add(v.parked_usdc, sub(equity, repay_amt)?)?;
            v.phoenix_equity_usdc = 0;
        }
        3 => {
            // Withdraw basis spot → Jupiter xStock→USDC → transit.
            let qty = v.basis_spot_qty;
            if qty > 0 {
                kamino::withdraw_collateral(qty)?;
                let usdc = jupiter::swap_stock_to_usdc(v, qty)?;
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

/// Unwinding(3) → Parked: clear any residual D_b, supply recovered USDC on Kamino.
pub fn unwind_commit(ctx: Context<KeeperVault>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::Unwinding)?;
    require_step(v, 3)?;
    if v.debt_b_usdc > 0 {
        let repay_amt = v.debt_b_usdc.min(v.parked_usdc);
        if repay_amt > 0 {
            kamino::repay_usdc(repay_amt)?;
            v.parked_usdc = sub(v.parked_usdc, repay_amt)?;
        }
        // Anything left is folded into the primary loan.
        v.debt_usdc = add(v.debt_usdc, sub(v.debt_b_usdc, repay_amt)?)?;
        v.debt_b_usdc = 0;
    }
    if v.parked_usdc > 0 {
        kamino::supply_usdc(v.parked_usdc)?;
    }
    check_ltv(v)?;
    set_state(v, VaultState::Parked, 0);
    recompute_nav(v)?;
    Ok(())
}

/// Basis → Basis: unwind `fraction_bps` of the basis position in one instruction.
pub fn unwind_partial(ctx: Context<KeeperVault>, fraction_bps: u32, reason: u8) -> Result<()> {
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
    let qty = mul_bps(v.basis_spot_qty, fraction_bps)?;
    let short_qty = mul_bps(v.phoenix_short_qty, fraction_bps)?;
    let equity_part = mul_bps(v.phoenix_equity_usdc, fraction_bps)?;
    let debt_b_part = mul_bps(v.debt_b_usdc, fraction_bps)?;

    if short_qty > 0 {
        let (filled, pnl) = phoenix::close_short(v, short_qty)?;
        v.phoenix_short_qty = sub(v.phoenix_short_qty, filled)?;
        v.phoenix_equity_usdc = apply_pnl(v.phoenix_equity_usdc, pnl)?;
    }
    let withdraw = equity_part.min(v.phoenix_equity_usdc);
    if withdraw > 0 {
        phoenix::withdraw_collateral(withdraw)?;
        v.phoenix_equity_usdc = sub(v.phoenix_equity_usdc, withdraw)?;
    }
    let repay_amt = withdraw.min(debt_b_part).min(v.debt_b_usdc);
    if repay_amt > 0 {
        kamino::repay_usdc(repay_amt)?;
        v.debt_b_usdc = sub(v.debt_b_usdc, repay_amt)?;
    }
    let mut recovered = sub(withdraw, repay_amt)?;
    if qty > 0 {
        kamino::withdraw_collateral(qty)?;
        let usdc = jupiter::swap_stock_to_usdc(v, qty)?;
        let min_out = mul_bps(stock_value(v, qty)?, 10_000 - v.params.max_swap_slippage_bps)?;
        require!(usdc >= min_out, CarreraError::SlippageExceeded);
        v.collateral_qty = sub(v.collateral_qty, qty)?;
        v.basis_spot_qty = sub(v.basis_spot_qty, qty)?;
        recovered = add(recovered, usdc)?;
    }
    if recovered > 0 {
        kamino::supply_usdc(recovered)?;
        v.parked_usdc = add(v.parked_usdc, recovered)?;
    }
    check_hedge(v)?;
    check_ltv(v)?;
    check_margin(v)?;
    recompute_nav(v)?;
    Ok(())
}

fn apply_pnl(equity: u64, pnl: i64) -> Result<u64> {
    let e = (equity as i128) + (pnl as i128);
    u64::try_from(e.max(0)).map_err(|_| error!(CarreraError::MathOverflow))
}

// ---------------------------------------------------------------- size up

/// Borrow up to target after deposits and deploy per mode. Keeper batches, never per deposit.
pub fn size_up(ctx: Context<KeeperVault>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    require_not_paused(&ctx.accounts.registry)?;
    let v = &mut ctx.accounts.vault;
    let st = v.vault_state();
    require!(st == VaultState::Parked || st == VaultState::Basis, CarreraError::WrongState);
    require_market_open(v)?;
    let ltv_now = ltv_bps(v)?;
    require!(
        ltv_now < v.params.ltv_bps.saturating_sub(v.params.size_band_bps),
        CarreraError::RuleNotSatisfied
    );
    let inc = borrow_primary_to_target(v)?;
    require!(inc > 0, CarreraError::RuleNotSatisfied);
    match st {
        VaultState::Parked => {
            kamino::supply_usdc(inc)?;
            v.parked_usdc = add(v.parked_usdc, inc)?;
        }
        VaultState::Basis => {
            let out = jupiter::swap_usdc_to_stock(v, inc)?;
            kamino::deposit_collateral(out)?;
            v.collateral_qty = add(v.collateral_qty, out)?;
            v.basis_spot_qty = add(v.basis_spot_qty, out)?;
            let d_b = mul_bps(inc, v.params.ltv_bps)?;
            kamino::borrow_usdc(d_b)?;
            phoenix::deposit_collateral(d_b)?;
            v.debt_b_usdc = add(v.debt_b_usdc, d_b)?;
            v.phoenix_equity_usdc = add(v.phoenix_equity_usdc, d_b)?;
            let filled = phoenix::open_short(v, out)?;
            v.phoenix_short_qty = add(v.phoenix_short_qty, filled)?;
            check_hedge(v)?;
            check_margin(v)?;
        }
        _ => unreachable!(),
    }
    check_ltv(v)?;
    recompute_nav(v)?;
    Ok(())
}

// ---------------------------------------------------------------- rebalancing

/// Basis, LTV > L + band: move free Phoenix collateral to Kamino and repay.
pub fn rebalance_to_kamino(ctx: Context<KeeperVault>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::Basis)?;
    require!(
        ltv_bps(v)? > v.params.ltv_bps + v.params.rebalance_ltv_band_bps,
        CarreraError::RuleNotSatisfied
    );
    v.phoenix_equity_usdc = phoenix::read_equity(v)?;
    let needed = sub(v.total_debt(), mul_bps(stock_value(v, v.collateral_qty)?, v.params.ltv_bps)?)?;
    let required_margin = mul_bps(stock_value(v, v.phoenix_short_qty)?, v.params.min_margin_bps)?;
    let free = v.phoenix_equity_usdc.saturating_sub(required_margin);
    let amount = needed.min(free);
    require!(amount > 0, CarreraError::MarginTooLow);
    phoenix::withdraw_collateral(amount)?;
    kamino::repay_usdc(amount)?;
    v.phoenix_equity_usdc = sub(v.phoenix_equity_usdc, amount)?;
    let from_b = amount.min(v.debt_b_usdc);
    v.debt_b_usdc = sub(v.debt_b_usdc, from_b)?;
    v.debt_usdc = sub(v.debt_usdc, sub(amount, from_b)?)?;
    check_margin(v)?;
    recompute_nav(v)?;
    emit!(Rebalanced { vault: vault_key(v), kind: REBALANCE_TO_KAMINO, amount });
    Ok(())
}

/// Basis, margin < min + band: borrow on Kamino and top up Phoenix.
pub fn rebalance_to_phoenix(ctx: Context<KeeperVault>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::Basis)?;
    v.phoenix_equity_usdc = phoenix::read_equity(v)?;
    let floor = v.params.min_margin_bps + v.params.rebalance_margin_band_bps;
    require!(margin_bps(v)? < floor, CarreraError::RuleNotSatisfied);
    let target = mul_bps(stock_value(v, v.phoenix_short_qty)?, floor)?;
    let amount = sub(target, v.phoenix_equity_usdc)?;
    require!(amount > 0, CarreraError::RuleNotSatisfied);
    kamino::borrow_usdc(amount)?;
    phoenix::deposit_collateral(amount)?;
    v.debt_b_usdc = add(v.debt_b_usdc, amount)?;
    v.phoenix_equity_usdc = add(v.phoenix_equity_usdc, amount)?;
    check_ltv(v)?;
    recompute_nav(v)?;
    emit!(Rebalanced { vault: vault_key(v), kind: REBALANCE_TO_PHOENIX, amount });
    Ok(())
}

/// Parked, LTV > L + band (or exits pending that need headroom, spec §8): withdraw supplied USDC and repay.
pub fn rebalance_from_parked(ctx: Context<KeeperVault>, amount: u64) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let v = &mut ctx.accounts.vault;
    require_state(v, VaultState::Parked)?;
    require!(
        v.pending_exit_shares > 0 || ltv_bps(v)? > v.params.ltv_bps + v.params.rebalance_ltv_band_bps,
        CarreraError::RuleNotSatisfied
    );
    let amount = amount.min(v.parked_usdc).min(v.debt_usdc);
    require!(amount > 0, CarreraError::InvalidArgument);
    kamino::withdraw_supplied_usdc(amount)?;
    kamino::repay_usdc(amount)?;
    v.parked_usdc = sub(v.parked_usdc, amount)?;
    v.debt_usdc = sub(v.debt_usdc, amount)?;
    recompute_nav(v)?;
    emit!(Rebalanced { vault: vault_key(v), kind: REBALANCE_FROM_PARKED, amount });
    Ok(())
}
