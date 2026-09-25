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
    pub fn refresh_nav<'info>(ctx: Context<'_, '_, '_, 'info, RefreshNav<'info>>, mock_price_e6: Option<u64>) -> Result<()> {
        instructions::refresh_nav(ctx, mock_price_e6)
    }
    /// Mock-only (see `instructions::oracle::mock_accrue`).
    pub fn mock_accrue(ctx: Context<KeeperVault>, usdc: u64, leg: u8) -> Result<()> {
        instructions::mock_accrue(ctx, usdc, leg)
    }

    // ---- engine (each takes the keeper's Borsh `VenueData` as `venue_data`; empty in mock builds)
    pub fn park<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, venue_data: Vec<u8>) -> Result<()> {
        instructions::park(ctx, venue_data)
    }
    pub fn repay<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, venue_data: Vec<u8>) -> Result<()> {
        instructions::repay(ctx, venue_data)
    }
    pub fn sync_collateral<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, venue_data: Vec<u8>) -> Result<()> {
        instructions::sync_collateral(ctx, venue_data)
    }
    pub fn wind_start<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, venue_data: Vec<u8>) -> Result<()> {
        instructions::wind_start(ctx, venue_data)
    }
    pub fn wind_step<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, n: u8, venue_data: Vec<u8>) -> Result<()> {
        instructions::wind_step(ctx, n, venue_data)
    }
    pub fn wind_commit<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, venue_data: Vec<u8>) -> Result<()> {
        instructions::wind_commit(ctx, venue_data)
    }
    pub fn wind_abort(ctx: Context<KeeperVault>) -> Result<()> {
        instructions::wind_abort(ctx)
    }
    pub fn unwind_start(ctx: Context<KeeperVault>, reason: u8) -> Result<()> {
        instructions::unwind_start(ctx, reason)
    }
    pub fn unwind_step<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, n: u8, venue_data: Vec<u8>) -> Result<()> {
        instructions::unwind_step(ctx, n, venue_data)
    }
    pub fn unwind_commit<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, venue_data: Vec<u8>) -> Result<()> {
        instructions::unwind_commit(ctx, venue_data)
    }
    pub fn unwind_partial<'info>(
        ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>,
        fraction_bps: u32,
        reason: u8,
        venue_data: Vec<u8>,
    ) -> Result<()> {
        instructions::unwind_partial(ctx, fraction_bps, reason, venue_data)
    }
    pub fn size_up<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, venue_data: Vec<u8>) -> Result<()> {
        instructions::size_up(ctx, venue_data)
    }
    pub fn rebalance_to_kamino<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, venue_data: Vec<u8>) -> Result<()> {
        instructions::rebalance_to_kamino(ctx, venue_data)
    }
    pub fn rebalance_to_phoenix<'info>(ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>, venue_data: Vec<u8>) -> Result<()> {
        instructions::rebalance_to_phoenix(ctx, venue_data)
    }
    pub fn rebalance_from_parked<'info>(
        ctx: Context<'_, '_, '_, 'info, KeeperVault<'info>>,
        amount: u64,
        venue_data: Vec<u8>,
    ) -> Result<()> {
        instructions::rebalance_from_parked(ctx, amount, venue_data)
    }
    pub fn init_kamino_obligation<'info>(
        ctx: Context<'_, '_, '_, 'info, InitKaminoObligation<'info>>,
        venue_data: Vec<u8>,
    ) -> Result<()> {
        instructions::init_kamino_obligation(ctx, venue_data)
    }

    // ---- epochs and fees
    pub fn close_epoch(ctx: Context<CloseEpoch>) -> Result<()> {
        instructions::close_epoch(ctx)
    }
    pub fn settle_epoch<'info>(ctx: Context<'_, '_, '_, 'info, SettleEpoch<'info>>, venue_data: Vec<u8>) -> Result<()> {
        instructions::settle_epoch(ctx, venue_data)
    }
    pub fn crystallise_fee(ctx: Context<CrystalliseFee>) -> Result<()> {
        instructions::crystallise_fee(ctx)
    }
}
