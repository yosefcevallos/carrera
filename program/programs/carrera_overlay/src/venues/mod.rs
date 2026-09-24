//! Venue adapters. Every external leg the program executes goes through one of
//! these functions. With `mock-venues` they simulate the leg from the vault's
//! cached oracle price and keeper-supplied values; without it they return
//! `VenueNotWired` until the real CPIs land (spec §7, M2/M3).
//!
//! Real wiring goes in the function bodies below; instruction signatures do not change.

pub mod hawkeye;
pub mod jupiter;
pub mod kamino;
pub mod phoenix;

#[cfg(feature = "mock-venues")]
pub const MOCK: bool = true;
#[cfg(not(feature = "mock-venues"))]
pub const MOCK: bool = false;
