//! Kamino Lend xStocks market CPIs: deposit/withdraw collateral, borrow/repay
//! USDC, supply/withdraw USDC, and reads of the reserve rates and the xStock
//! oracle price. The obligation is Kamino-derived for the vault PDA (spec §4.1).

use crate::errors::CarreraError;
use anchor_lang::prelude::*;

fn wired() -> Result<()> {
    if super::MOCK {
        Ok(())
    } else {
        err!(CarreraError::VenueNotWired)
    }
}

pub fn deposit_collateral(_qty: u64) -> Result<()> {
    wired()
}
pub fn withdraw_collateral(_qty: u64) -> Result<()> {
    wired()
}
pub fn borrow_usdc(_amount: u64) -> Result<()> {
    wired()
}
pub fn repay_usdc(_amount: u64) -> Result<()> {
    wired()
}
pub fn supply_usdc(_amount: u64) -> Result<()> {
    wired()
}
pub fn withdraw_supplied_usdc(_amount: u64) -> Result<()> {
    wired()
}

/// (borrow_apy_bps, supply_apy_bps) of the USDC reserve. In mock builds the
/// keeper-supplied values are used; otherwise decode `reserve`.
pub fn read_rates(_reserve: &AccountInfo, mock: Option<(u32, u32)>) -> Result<(u32, u32)> {
    match (super::MOCK, mock) {
        (true, Some(v)) => Ok(v),
        (true, None) => err!(CarreraError::InvalidArgument),
        (false, Some(_)) => err!(CarreraError::MockNotAllowed),
        (false, None) => err!(CarreraError::VenueNotWired),
    }
}

/// xStock price, USD × 1e6 per whole unit, from the oracle Kamino uses for the reserve.
pub fn read_price(_oracle: &AccountInfo, mock: Option<u64>) -> Result<u64> {
    match (super::MOCK, mock) {
        (true, Some(p)) if p > 0 => Ok(p),
        (true, _) => err!(CarreraError::InvalidArgument),
        (false, Some(_)) => err!(CarreraError::MockNotAllowed),
        (false, None) => err!(CarreraError::VenueNotWired),
    }
}
