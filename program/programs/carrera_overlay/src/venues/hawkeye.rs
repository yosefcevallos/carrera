//! Hawkeye view program: `view_funding` for the vault's market. Returns the
//! hourly funding rate to shorts in bps × 1e6.

use crate::errors::CarreraError;
use anchor_lang::prelude::*;

pub fn read_funding(_view: &AccountInfo, mock: Option<i64>) -> Result<i64> {
    match (super::MOCK, mock) {
        (true, Some(v)) => Ok(v),
        (true, None) => err!(CarreraError::InvalidArgument),
        (false, Some(_)) => err!(CarreraError::MockNotAllowed),
        (false, None) => err!(CarreraError::VenueNotWired),
    }
}
