pub mod admin;
pub mod engine;
pub mod epoch;
pub mod oracle;
pub mod user;
pub mod venue_setup;

pub use admin::*;
pub use engine::*;
pub use epoch::*;
pub use oracle::*;
pub use user::*;
pub use venue_setup::*;

use crate::errors::CarreraError;
use crate::events::{RuleEvaluated, StateChanged};
use crate::nav;
use crate::ring;
use crate::rule::{self, Decision, RuleInputs};
use crate::state::{OverlayVault, Registry, VaultState};
use anchor_lang::prelude::*;

/// Debt at or below this (USDC base units, 0.01 USDC) is dust: it never blocks
/// settlement and is written off when a repay cannot cover it. A constant rather
/// than a `VaultParams` field so the live vault accounts keep their layout.
pub const DEBT_DUST_USDC: u64 = 10_000;

/// Total debt with dust treated as zero (used for settlement LTV and NAV events).
pub fn effective_debt(vault: &OverlayVault) -> u64 {
    let d = vault.total_debt();
    if d < DEBT_DUST_USDC {
        0
    } else {
        d
    }
}

/// Write off residual debt below the dust threshold after a repay.
pub fn write_off_dust(vault: &mut OverlayVault) {
    if vault.total_debt() < DEBT_DUST_USDC {
        vault.debt_usdc = 0;
        vault.debt_b_usdc = 0;
    }
}

/// Shared accounts for keeper crank instructions.
#[derive(Accounts)]
pub struct KeeperVault<'info> {
    pub keeper: Signer<'info>,
    #[account(seeds = [b"registry"], bump = registry.bump)]
    pub registry: Account<'info, Registry>,
    #[account(mut, seeds = [b"vault", vault.xstock_mint.as_ref()], bump = vault.bump)]
    pub vault: Account<'info, OverlayVault>,
}

pub fn require_keeper(registry: &Registry, signer: &Pubkey) -> Result<()> {
    require!(registry.is_keeper(signer), CarreraError::Unauthorized);
    Ok(())
}

pub fn require_keeper_or_guardian(registry: &Registry, signer: &Pubkey) -> Result<()> {
    require!(
        registry.is_keeper(signer) || registry.guardian == *signer,
        CarreraError::Unauthorized
    );
    Ok(())
}

pub fn require_not_paused(registry: &Registry) -> Result<()> {
    require!(!registry.paused, CarreraError::Paused);
    Ok(())
}

pub fn require_state(vault: &OverlayVault, state: VaultState) -> Result<()> {
    require!(vault.vault_state() == state, CarreraError::WrongState);
    Ok(())
}

pub fn require_step(vault: &OverlayVault, step: u8) -> Result<()> {
    require!(vault.step == step, CarreraError::WrongStep);
    Ok(())
}

pub fn require_market_open(vault: &OverlayVault) -> Result<()> {
    require!(vault.market_open, CarreraError::MarketClosed);
    Ok(())
}

pub fn require_nav_fresh(vault: &OverlayVault) -> Result<()> {
    let slot = Clock::get()?.slot;
    require!(vault.nav_slot > 0, CarreraError::NavStale);
    require!(
        slot.saturating_sub(vault.nav_slot) <= vault.params.max_nav_age_slots,
        CarreraError::NavStale
    );
    Ok(())
}

pub fn set_state(vault: &mut OverlayVault, to: VaultState, step: u8) {
    let from = vault.state;
    vault.state = to as u8;
    vault.step = step;
    emit!(StateChanged { vault: vault_key(vault), from, to: to as u8, step });
}

/// The vault PDA key, recomputed from seeds (avoids threading the account key everywhere).
pub fn vault_key(vault: &OverlayVault) -> Pubkey {
    Pubkey::create_program_address(
        &[b"vault", vault.xstock_mint.as_ref(), &[vault.bump]],
        &crate::ID,
    )
    .unwrap_or_default()
}

pub fn nav_inputs(vault: &OverlayVault) -> nav::NavInputs {
    nav::NavInputs {
        collateral_qty: vault.collateral_qty,
        basis_spot_qty: vault.basis_spot_qty,
        debt_usdc: vault.debt_usdc,
        debt_b_usdc: vault.debt_b_usdc,
        phoenix_equity_usdc: vault.phoenix_equity_usdc,
        parked_usdc: vault.parked_usdc,
        price_e6: vault.price_e6,
        decimals: vault.stock_decimals,
        total_shares: vault.total_shares,
    }
}

/// Recompute the NAV cache from the current accounting at the cached price.
/// Does not touch `nav_slot`; only `refresh_nav` (a fresh oracle read) does.
pub fn recompute_nav(vault: &mut OverlayVault) -> Result<nav::Nav> {
    let n = nav::compute_nav(&nav_inputs(vault)).ok_or_else(|| error!(CarreraError::MathOverflow))?;
    vault.nav_usd_e6 = n.nav_usdc;
    vault.share_price_stock_e6 = n.share_price_stock_e6;
    Ok(n)
}

pub fn depositor_qty(vault: &OverlayVault) -> u64 {
    vault.collateral_qty.saturating_sub(vault.basis_spot_qty)
}

pub fn stock_value(vault: &OverlayVault, qty: u64) -> Result<u64> {
    nav::stock_value_usdc(qty, vault.price_e6, vault.stock_decimals)
        .ok_or_else(|| error!(CarreraError::MathOverflow))
}

pub fn mul_bps(amount: u64, bps: u32) -> Result<u64> {
    nav::mul_bps(amount, bps).ok_or_else(|| error!(CarreraError::MathOverflow))
}

/// total debt / collateral value, bps.
pub fn ltv_bps(vault: &OverlayVault) -> Result<u32> {
    Ok(nav::ratio_bps(vault.total_debt(), stock_value(vault, vault.collateral_qty)?))
}

/// Phoenix equity / short notional, bps. No short → u32::MAX (no margin constraint).
pub fn margin_bps(vault: &OverlayVault) -> Result<u32> {
    if vault.phoenix_short_qty == 0 {
        return Ok(u32::MAX);
    }
    Ok(nav::ratio_bps(vault.phoenix_equity_usdc, stock_value(vault, vault.phoenix_short_qty)?))
}

pub fn check_ltv(vault: &OverlayVault) -> Result<()> {
    require!(ltv_bps(vault)? <= vault.params.ltv_bps, CarreraError::LtvTooHigh);
    Ok(())
}

pub fn check_margin(vault: &OverlayVault) -> Result<()> {
    require!(margin_bps(vault)? >= vault.params.min_margin_bps, CarreraError::MarginTooLow);
    Ok(())
}

/// `lot` is the Phoenix base-lot size in stock base units (`VenueData::base_lot_size`; 0 on mock
/// builds): the spot's sub-lot remainder cannot be shorted and is tolerated as unhedged dust.
pub fn check_hedge(vault: &OverlayVault, lot: u64) -> Result<()> {
    require!(
        nav::hedge_ok(vault.basis_spot_qty, vault.phoenix_short_qty, vault.params.hedge_tol_bps, lot),
        CarreraError::HedgeOutOfTolerance
    );
    Ok(())
}

/// Evaluate the allocation rule, cache it on the vault and emit `RuleEvaluated`.
pub fn evaluate_rule(registry: &Registry, vault: &mut OverlayVault) -> Result<Decision> {
    let f_avg = ring::f_avg_bps(&vault.funding, vault.funding_samples);
    let f_3h = ring::mean_last(&vault.funding, vault.funding_head, vault.funding_samples, rule::ENTRY_WINDOW as usize);
    let inputs = RuleInputs {
        state: vault.vault_state(),
        f_avg_bps: f_avg,
        f_3h_bps: f_3h,
        samples: vault.funding_samples,
        s_bps: registry.supply_apy_bps,
        r_bps: registry.borrow_apy_bps,
        market_open: vault.market_open,
        paused: registry.paused,
    };
    let out = rule::evaluate(&vault.params, &inputs);
    let now = Clock::get()?.unix_timestamp;
    vault.last_rule = crate::state::RuleEvaluation {
        f_avg_bps: out.f_avg_bps,
        parked_apy_bps: registry.supply_apy_bps,
        r_bps: registry.borrow_apy_bps,
        hurdle_bps: out.hurdle_bps,
        decision: out.decision.as_u8(),
        ts: now,
    };
    emit!(RuleEvaluated {
        vault: vault_key(vault),
        f_avg_bps: out.f_avg_bps,
        parked_apy_bps: registry.supply_apy_bps,
        r_bps: registry.borrow_apy_bps,
        hurdle_bps: out.hurdle_bps,
        decision: out.decision.as_u8(),
        f_3h_bps: out.f_3h_bps,
        be_bps: out.be_bps,
    });
    Ok(out.decision)
}
