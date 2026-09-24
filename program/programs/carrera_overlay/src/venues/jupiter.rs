//! Jupiter v6 `shared_accounts_route` CPI (spec §7.4). Real implementation must
//! check the out-mint, derive `min_out` from the Kamino oracle × slippage bound,
//! and verify route accounts belong to the whitelisted Jupiter program.

use crate::errors::CarreraError;
use crate::nav;
use crate::state::OverlayVault;
use anchor_lang::prelude::*;

/// Swap `usdc_in` for the vault's xStock. Returns stock base units received.
pub fn swap_usdc_to_stock(vault: &OverlayVault, usdc_in: u64) -> Result<u64> {
    if !super::MOCK {
        return err!(CarreraError::VenueNotWired);
    }
    nav::stock_qty_from_usdc(usdc_in, vault.price_e6, vault.stock_decimals)
        .ok_or_else(|| error!(CarreraError::MathOverflow))
}

/// Swap `qty` xStock for USDC. Returns USDC base units received.
pub fn swap_stock_to_usdc(vault: &OverlayVault, qty: u64) -> Result<u64> {
    if !super::MOCK {
        return err!(CarreraError::VenueNotWired);
    }
    nav::stock_value_usdc(qty, vault.price_e6, vault.stock_decimals)
        .ok_or_else(|| error!(CarreraError::MathOverflow))
}
