//! Carrera xStock funding overlay vaults.
//!
//! See `docs/handover/01-one-pager-and-spec-v0.4.md`, `docs/DECISIONS.md` and
//! `docs/CONTRACT.md` at the repository root.

use anchor_lang::prelude::*;

pub mod errors;
pub mod events;
pub mod instructions;
pub mod nav;
pub mod ring;
pub mod rule;
pub mod state;
pub mod venues;

use instructions::*;
use state::VaultParams;

declare_id!("GH45ANLzg1t6rNnaoNqE39rN1rKGXFQXPZnNxvbhUmYw");

#[program]
pub mod carrera_overlay {
    use super::*;

    // ---- admin
    pub fn init_registry(ctx: Context<InitRegistry>, guardian: Pubkey, keepers: Vec<Pubkey>) -> Result<()> {
        instructions::init_registry(ctx, guardian, keepers)
    }
    pub fn set_roles(
        ctx: Context<AdminRegistry>,
        admin: Option<Pubkey>,
        guardian: Option<Pubkey>,
        keepers: Option<Vec<Pubkey>>,
    ) -> Result<()> {
        instructions::set_roles(ctx, admin, guardian, keepers)
    }
    pub fn pause(ctx: Context<PauseRegistry>) -> Result<()> {
        instructions::pause(ctx)
    }
    pub fn unpause(ctx: Context<PauseRegistry>) -> Result<()> {
        instructions::unpause(ctx)
    }
    pub fn init_vault(ctx: Context<InitVault>, tier: u8, params: VaultParams) -> Result<()> {
        instructions::init_vault(ctx, tier, params)
    }
    pub fn set_params(ctx: Context<AdminVault>, params: VaultParams) -> Result<()> {
        instructions::set_params(ctx, params)
    }
    pub fn set_market_open(ctx: Context<KeeperVault>, open: bool) -> Result<()> {
        instructions::set_market_open(ctx, open)
    }

    // ---- user
    pub fn deposit(ctx: Context<Deposit>, qty: u64, min_shares: u64) -> Result<()> {
        instructions::deposit(ctx, qty, min_shares)
    }
    pub fn request_exit(ctx: Context<RequestExit>, shares: u64, nonce: u64) -> Result<()> {
        instructions::request_exit(ctx, shares, nonce)
    }
    pub fn cancel_exit(ctx: Context<CancelExit>) -> Result<()> {
        instructions::cancel_exit(ctx)
    }
    pub fn redeem(ctx: Context<Redeem>) -> Result<()> {
        instructions::redeem(ctx)
    }

    // ---- data
    pub fn record_funding(ctx: Context<RecordFunding>, mock_rate_bps_hourly: Option<i64>) -> Result<()> {
        instructions::record_funding(ctx, mock_rate_bps_hourly)
    }
    pub fn record_kamino_rates(
        ctx: Context<RecordKaminoRates>,
        mock_borrow_bps: Option<u32>,
        mock_supply_bps: Option<u32>,
    ) -> Result<()> {
        instructions::record_kamino_rates(ctx, mock_borrow_bps, mock_supply_bps)
    }
    pub fn refresh_nav(ctx: Context<RefreshNav>, mock_price_e6: Option<u64>) -> Result<()> {
        instructions::refresh_nav(ctx, mock_price_e6)
    }
    /// Mock-only (see `instructions::oracle::mock_accrue`).
    pub fn mock_accrue(ctx: Context<KeeperVault>, usdc: u64, leg: u8) -> Result<()> {
        instructions::mock_accrue(ctx, usdc, leg)
    }

    // ---- engine
    pub fn park(ctx: Context<KeeperVault>) -> Result<()> {
        instructions::park(ctx)
    }
    pub fn repay(ctx: Context<KeeperVault>) -> Result<()> {
        instructions::repay(ctx)
    }
    pub fn wind_start(ctx: Context<KeeperVault>) -> Result<()> {
        instructions::wind_start(ctx)
    }
    pub fn wind_step(ctx: Context<KeeperVault>, n: u8) -> Result<()> {
        instructions::wind_step(ctx, n)
    }
    pub fn wind_commit(ctx: Context<KeeperVault>) -> Result<()> {
        instructions::wind_commit(ctx)
    }
    pub fn wind_abort(ctx: Context<KeeperVault>) -> Result<()> {
        instructions::wind_abort(ctx)
    }
    pub fn unwind_start(ctx: Context<KeeperVault>, reason: u8) -> Result<()> {
        instructions::unwind_start(ctx, reason)
    }
    pub fn unwind_step(ctx: Context<KeeperVault>, n: u8) -> Result<()> {
        instructions::unwind_step(ctx, n)
    }
    pub fn unwind_commit(ctx: Context<KeeperVault>) -> Result<()> {
        instructions::unwind_commit(ctx)
    }
    pub fn unwind_partial(ctx: Context<KeeperVault>, fraction_bps: u32, reason: u8) -> Result<()> {
        instructions::unwind_partial(ctx, fraction_bps, reason)
    }
    pub fn size_up(ctx: Context<KeeperVault>) -> Result<()> {
        instructions::size_up(ctx)
    }
    pub fn rebalance_to_kamino(ctx: Context<KeeperVault>) -> Result<()> {
        instructions::rebalance_to_kamino(ctx)
    }
    pub fn rebalance_to_phoenix(ctx: Context<KeeperVault>) -> Result<()> {
        instructions::rebalance_to_phoenix(ctx)
    }
    pub fn rebalance_from_parked(ctx: Context<KeeperVault>, amount: u64) -> Result<()> {
        instructions::rebalance_from_parked(ctx, amount)
    }

    // ---- epochs and fees
    pub fn close_epoch(ctx: Context<CloseEpoch>) -> Result<()> {
        instructions::close_epoch(ctx)
    }
    pub fn settle_epoch(ctx: Context<SettleEpoch>) -> Result<()> {
        instructions::settle_epoch(ctx)
    }
    pub fn crystallise_fee(ctx: Context<CrystalliseFee>) -> Result<()> {
        instructions::crystallise_fee(ctx)
    }
}
