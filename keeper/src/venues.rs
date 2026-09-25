//! Where the hourly inputs come from.
//!
//! Onchain: the keeper passes `None` for every mock argument and the program reads
//! funding from the Hawkeye view account, rates from the Kamino reserve and price
//! from the oracle account in the same transaction. The keeper only supplies the
//! account addresses (config). Needs a program built without `mock-venues`.
//!
//! Live: the keeper fetches funding, rates and prices from public APIs (see
//! `feed.rs`) and passes them as `Some(..)`. Needs a program built with `mock-venues`.
//!
//! Mock: a JSON file supplies the same values; for localnet. Re-read every pass.

use crate::feed::{FeedView, LiveFeed};
use anyhow::{Context, Result};
use chrono::Utc;
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
    Onchain,
    Mock { path: PathBuf, data: MockFile },
    Live(LiveFeed),
}

impl Venues {
    pub fn onchain() -> Self {
        Self::Onchain
    }

    pub fn mock(path: PathBuf) -> Result<Self> {
        let data = Self::read(&path)?;
        Ok(Self::Mock { path, data })
    }

    pub fn live(feed: LiveFeed) -> Self {
        Self::Live(feed)
    }

    fn read(path: &PathBuf) -> Result<MockFile> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading mock file {}", path.display()))?;
        serde_json::from_str(&text).context("parsing mock file")
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Onchain => "onchain",
            Self::Mock { .. } => "mock",
            Self::Live(_) => "live",
        }
    }

    /// True when the keeper supplies the values (mock or live). A `None` from
    /// `funding`/`price`/`rates` then means "no value this pass, skip the crank".
    pub fn supplies_values(&self) -> bool {
        !matches!(self, Self::Onchain)
    }

    /// Refresh inputs: re-read the mock file, or fetch the live feed. No-op when onchain.
    pub async fn reload(&mut self) {
        match self {
            Self::Onchain => {}
            Self::Mock { path, data } => match Self::read(path) {
                Ok(d) => *data = d,
                Err(e) => tracing::warn!("mock file reload failed, keeping previous values: {e:#}"),
            },
            Self::Live(feed) => {
                feed.refresh(Utc::now()).await;
                let s = &feed.snapshot;
                tracing::info!(
                    "feed: kamino borrow={:?} supply={:?}; {}",
                    s.rates.as_ref().map(|r| r.borrow_bps),
                    s.rates.as_ref().map(|r| r.supply_bps),
                    s.vaults
                        .iter()
                        .map(|(k, v)| format!("{k}: funding={:?} price_e6={:?} open={:?}", v.funding_hourly_scaled, v.price_e6, v.phoenix_open))
                        .collect::<Vec<_>>()
                        .join("; ")
                );
            }
        }
    }

    /// Live feed only: reload Jupiter prices when any are missing or older than
    /// `max_age_secs`. Mock and onchain feeds never need it.
    pub async fn ensure_prices(&mut self, max_age_secs: i64) {
        if let Self::Live(feed) = self {
            let now = Utc::now();
            if feed.prices_stale(now, max_age_secs) {
                feed.refresh_prices(now).await;
            }
        }
    }

    pub fn funding(&self, symbol: &str) -> Option<i64> {
        match self {
            Self::Onchain => None,
            Self::Mock { data, .. } => data.vaults.get(symbol).map(|v| v.funding_hourly_bps_e6),
            Self::Live(f) => f.snapshot.vaults.get(symbol).and_then(|v| v.funding_hourly_scaled),
        }
    }

    pub fn price(&self, symbol: &str) -> Option<u64> {
        match self {
            Self::Onchain => None,
            Self::Mock { data, .. } => data.vaults.get(symbol).map(|v| v.price_e6),
            Self::Live(f) => f.snapshot.vaults.get(symbol).and_then(|v| v.price_e6),
        }
    }

    pub fn rates(&self) -> (Option<u32>, Option<u32>) {
        match self {
            Self::Onchain => (None, None),
            Self::Mock { data, .. } => (Some(data.borrow_apy_bps), Some(data.supply_apy_bps)),
            Self::Live(f) => match &f.snapshot.rates {
                Some(r) => (Some(r.borrow_bps), Some(r.supply_bps)),
                None => (None, None),
            },
        }
    }

    /// Phoenix's own view of whether the market is open (live feed only).
    pub fn phoenix_open(&self, symbol: &str) -> Option<bool> {
        match self {
            Self::Live(f) => f.snapshot.vaults.get(symbol).and_then(|v| v.phoenix_open),
            _ => None,
        }
    }

    pub fn feed_view(&self, symbol: &str) -> Option<FeedView> {
        match self {
            Self::Live(f) => f.snapshot.vaults.get(symbol).cloned(),
            _ => None,
        }
    }
}
