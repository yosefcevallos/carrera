//! Where the hourly inputs come from.
//!
//! Live: the keeper passes `None` for every mock argument and the program reads
//! funding from the Hawkeye view account, rates from the Kamino reserve and price
//! from the oracle account in the same transaction. The keeper only supplies the
//! account addresses (config).
//!
//! Mock: a JSON file supplies hourly funding, Kamino rates and prices, and the
//! keeper passes them as `Some(..)`. Only a program built with `mock-venues`
//! accepts this. The file is re-read every pass so it can be edited while running.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct MockVault {
    /// Hourly funding rate paid to shorts, in bps × 1e6 (0.4 bps/h == 400_000).
    pub funding_hourly_bps_e6: i64,
    /// Stock price in USD with 6 decimals.
    pub price_e6: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MockFile {
    pub borrow_apy_bps: u32,
    pub supply_apy_bps: u32,
    pub vaults: BTreeMap<String, MockVault>,
}

pub enum Venues {
    Live,
    Mock { path: PathBuf, data: MockFile },
}

impl Venues {
    pub fn live() -> Self {
        Self::Live
    }

    pub fn mock(path: PathBuf) -> Result<Self> {
        let data = Self::read(&path)?;
        Ok(Self::Mock { path, data })
    }

    fn read(path: &PathBuf) -> Result<MockFile> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading mock file {}", path.display()))?;
        serde_json::from_str(&text).context("parsing mock file")
    }

    pub fn is_mock(&self) -> bool {
        matches!(self, Self::Mock { .. })
    }

    /// Re-read the mock file; no-op when live.
    pub fn reload(&mut self) {
        if let Self::Mock { path, data } = self {
            match Self::read(path) {
                Ok(d) => *data = d,
                Err(e) => tracing::warn!("mock file reload failed, keeping previous values: {e:#}"),
            }
        }
    }

    pub fn funding(&self, symbol: &str) -> Option<i64> {
        match self {
            Self::Live => None,
            Self::Mock { data, .. } => data.vaults.get(symbol).map(|v| v.funding_hourly_bps_e6),
        }
    }

    pub fn price(&self, symbol: &str) -> Option<u64> {
        match self {
            Self::Live => None,
            Self::Mock { data, .. } => data.vaults.get(symbol).map(|v| v.price_e6),
        }
    }

    pub fn rates(&self) -> (Option<u32>, Option<u32>) {
        match self {
            Self::Live => (None, None),
            Self::Mock { data, .. } => (Some(data.borrow_apy_bps), Some(data.supply_apy_bps)),
        }
    }
}
