//! Funding input. Per DECISIONS D6 the hourly funding rate is keeper-supplied on
//! every build (the `record_funding` handler restricts the value to registered
//! keepers); direct reads of Phoenix accounts through `phoenix-rise` layouts are a
//! documented future option. Unit: hourly rate to shorts in bps × 1e6.

use crate::errors::CarreraError;
use anchor_lang::prelude::*;

pub fn read_funding(_view: &AccountInfo, supplied: Option<i64>) -> Result<i64> {
    supplied.ok_or_else(|| error!(CarreraError::InvalidArgument))
}
