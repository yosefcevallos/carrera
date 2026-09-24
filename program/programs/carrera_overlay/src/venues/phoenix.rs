//! Phoenix perps CPIs via `phoenix-rise` (spec §7.4): Ember wrap/unwrap and
//! deposit/withdraw of USDC collateral into the vault's isolated subaccount,
//! bounded market orders with `last_valid_slot`, and Hawkeye `view_margin`.

use crate::errors::CarreraError;
use crate::state::OverlayVault;
use anchor_lang::prelude::*;

fn wired() -> Result<()> {
    if super::MOCK {
        Ok(())
    } else {
        err!(CarreraError::VenueNotWired)
    }
}

pub fn deposit_collateral(_amount: u64) -> Result<()> {
    wired()
}
pub fn withdraw_collateral(_amount: u64) -> Result<()> {
    wired()
}

/// Open a short of `qty` base lots. Returns the filled quantity. Real
/// implementation checks return-data fill ≥ (1 − max_perp_slippage) × qty and
/// fill price within `max_index_dev_bps` of index.
pub fn open_short(_vault: &OverlayVault, qty: u64) -> Result<u64> {
    wired()?;
    Ok(qty)
}

/// Close `qty` of the short (reduce-only). Returns realised PnL in USDC (mock: 0).
pub fn close_short(_vault: &OverlayVault, qty: u64) -> Result<(u64, i64)> {
    wired()?;
    Ok((qty, 0))
}

/// Subaccount equity: collateral + unrealised PnL + pending funding (Hawkeye `view_margin`).
pub fn read_equity(vault: &OverlayVault) -> Result<u64> {
    wired()?;
    Ok(vault.phoenix_equity_usdc)
}
