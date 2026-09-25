//! Keeper configuration: a TOML file with a few env overrides.

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use solana_sdk::pubkey::Pubkey;
use std::{collections::BTreeMap, path::PathBuf, str::FromStr};

#[derive(Debug, Clone, Deserialize)]
pub struct RawVault {
    pub mint: String,
    /// Decimals of the xStock mint. Used only for the keeper's own LTV / margin estimates.
    #[serde(default = "default_decimals")]
    pub stock_decimals: u8,
    /// Hawkeye view account passed to `record_funding` (ignored under --mock).
    #[serde(default)]
    pub hawkeye_view: Option<String>,
    /// Price oracle account passed to `refresh_nav` (ignored under --mock).
    #[serde(default)]
    pub oracle: Option<String>,
    /// Phoenix perp market symbol for the live feed (default: the vault symbol).
    #[serde(default)]
    pub phoenix_market: Option<String>,
    /// Token program that owns the xStock mint (default: Token-2022, as on mainnet).
    #[serde(default)]
    pub stock_token_program: Option<String>,
    /// Kamino reserve for this xStock in the xStocks market (informational; the rule uses the USDC reserve).
    #[serde(default)]
    pub kamino_reserve: Option<String>,
}

fn default_decimals() -> u8 {
    8
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawConfig {
    pub rpc_url: String,
    pub keypair_path: String,
    pub program_id: String,
    pub usdc_mint: String,
    /// Kamino USDC reserve passed to `record_kamino_rates` (ignored under --mock).
    #[serde(default)]
    pub kamino_reserve: Option<String>,
    /// Token account that receives performance-fee shares. Fees are skipped when unset.
    #[serde(default)]
    pub treasury_shares: Option<String>,
    #[serde(default = "default_hourly")]
    pub hourly_interval_secs: u64,
    #[serde(default = "default_fast")]
    pub fast_interval_secs: u64,
    /// Settlement pass period (exit epochs): unwind / repay as needed, then close + settle.
    #[serde(default = "default_settle")]
    pub settle_interval_secs: u64,
    /// "auto" (NYSE calendar AND Phoenix state), "open" (force the on-chain flag true and
    /// never flip it false; bypasses spec §7.5, demo only), "closed" (force false).
    #[serde(default = "default_market_open")]
    pub market_open: String,
    #[serde(default)]
    pub alert_webhook_url: Option<String>,
    #[serde(default = "default_min_sol")]
    pub min_keeper_sol: f64,
    #[serde(default = "default_lease_path")]
    pub lease_path: String,
    #[serde(default = "default_lease_ttl")]
    pub lease_ttl_secs: u64,
    /// Bind address of the HTTP status server.
    #[serde(default = "default_status_bind")]
    pub status_bind: String,
    /// Optional JSONL file that every fast-loop history sample is appended to.
    #[serde(default)]
    pub history_path: Option<String>,
    /// Where hourly inputs come from: "onchain" (program reads venues), "live" (public APIs), "mock" (JSON file).
    #[serde(default = "default_feed")]
    pub feed: String,
    #[serde(default = "default_phoenix_api")]
    pub phoenix_api_url: String,
    #[serde(default = "default_kamino_api")]
    pub kamino_api_url: String,
    /// Kamino lending market whose USDC reserve sets borrow/supply rates.
    #[serde(default = "default_kamino_market")]
    pub kamino_market: String,
    #[serde(default = "default_jupiter_price")]
    pub jupiter_price_url: String,
    /// Mock JSON file, used when feed = "mock" (or `--mock` on the CLI).
    #[serde(default)]
    pub mock_path: Option<String>,
    /// symbol -> vault. Nine entries in production.
    pub vaults: BTreeMap<String, RawVault>,
}

fn default_feed() -> String {
    "onchain".into()
}
fn default_phoenix_api() -> String {
    "https://perp-api.phoenix.trade".into()
}
fn default_kamino_api() -> String {
    "https://api.kamino.finance".into()
}
fn default_kamino_market() -> String {
    "5wJeMrUYECGq41fxRESKALVcHnNX26TAWy4W98yULsua".into()
}
fn default_jupiter_price() -> String {
    "https://lite-api.jup.ag/price/v3".into()
}

fn default_hourly() -> u64 {
    3600
}
fn default_fast() -> u64 {
    60
}
fn default_settle() -> u64 {
    300
}
fn default_market_open() -> String {
    "auto".into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketOpenMode {
    Auto,
    Open,
    Closed,
}

impl MarketOpenMode {
    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Self::Auto,
            "open" => Self::Open,
            "closed" => Self::Closed,
            other => return Err(anyhow!("market_open must be auto | open | closed, got {other:?}")),
        })
    }
}
fn default_min_sol() -> f64 {
    0.5
}
fn default_lease_path() -> String {
    "/tmp/carrera-keeper.lease".into()
}
fn default_lease_ttl() -> u64 {
    90
}
fn default_status_bind() -> String {
    "127.0.0.1:8787".into()
}

#[derive(Debug, Clone)]
pub struct VaultCfg {
    pub symbol: String,
    pub mint: Pubkey,
    pub stock_decimals: u8,
    pub hawkeye_view: Pubkey,
    pub oracle: Pubkey,
    pub phoenix_market: String,
    pub stock_token_program: Pubkey,
    pub kamino_reserve: Option<Pubkey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feed {
    Onchain,
    Live,
    Mock,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub rpc_url: String,
    pub keypair_path: PathBuf,
    pub program_id: Pubkey,
    pub usdc_mint: Pubkey,
    pub kamino_reserve: Pubkey,
    pub treasury_shares: Option<Pubkey>,
    pub hourly_interval_secs: u64,
    pub fast_interval_secs: u64,
    pub settle_interval_secs: u64,
    pub market_open: MarketOpenMode,
    pub alert_webhook_url: Option<String>,
    pub min_keeper_sol: f64,
    pub lease_path: PathBuf,
    pub lease_ttl_secs: u64,
    pub status_bind: String,
    pub history_path: Option<PathBuf>,
    pub feed: Feed,
    pub phoenix_api_url: String,
    pub kamino_api_url: String,
    pub kamino_market: String,
    pub jupiter_price_url: String,
    pub mock_path: Option<PathBuf>,
    pub vaults: Vec<VaultCfg>,
}

fn pk(s: &str, what: &str) -> Result<Pubkey> {
    Pubkey::from_str(s).with_context(|| format!("bad pubkey for {what}: {s}"))
}

fn pk_opt(s: &Option<String>, what: &str) -> Result<Pubkey> {
    match s {
        Some(v) => pk(v, what),
        None => Ok(Pubkey::default()),
    }
}

impl Config {
    /// Load from `path` (or `$CARRERA_KEEPER_CONFIG`, or `keeper.toml`), then apply env overrides
    /// `CARRERA_RPC_URL`, `CARRERA_KEYPAIR`, `CARRERA_PROGRAM_ID`, `CARRERA_ALERT_WEBHOOK`.
    pub fn load(path: Option<PathBuf>) -> Result<Self> {
        let path = path
            .or_else(|| std::env::var("CARRERA_KEEPER_CONFIG").ok().map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("keeper.toml"));
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let mut raw: RawConfig = toml::from_str(&text).context("parsing config")?;
        if let Ok(v) = std::env::var("CARRERA_RPC_URL") {
            raw.rpc_url = v;
        }
        if let Ok(v) = std::env::var("CARRERA_KEYPAIR") {
            raw.keypair_path = v;
        }
        if let Ok(v) = std::env::var("CARRERA_PROGRAM_ID") {
            raw.program_id = v;
        }
        if let Ok(v) = std::env::var("CARRERA_ALERT_WEBHOOK") {
            raw.alert_webhook_url = Some(v);
        }
        Self::from_raw(raw)
    }

    pub fn from_raw(raw: RawConfig) -> Result<Self> {
        if raw.vaults.is_empty() {
            return Err(anyhow!("config has no vaults"));
        }
        let mut vaults = Vec::with_capacity(raw.vaults.len());
        for (symbol, v) in &raw.vaults {
            vaults.push(VaultCfg {
                symbol: symbol.clone(),
                mint: pk(&v.mint, symbol)?,
                stock_decimals: v.stock_decimals,
                hawkeye_view: pk_opt(&v.hawkeye_view, "hawkeye_view")?,
                oracle: pk_opt(&v.oracle, "oracle")?,
                phoenix_market: v.phoenix_market.clone().unwrap_or_else(|| symbol.clone()),
                stock_token_program: match &v.stock_token_program {
                    Some(s) => pk(s, "stock_token_program")?,
                    None => crate::ix::TOKEN_2022_PROGRAM_ID,
                },
                kamino_reserve: match &v.kamino_reserve {
                    Some(s) => Some(pk(s, "kamino_reserve")?),
                    None => None,
                },
            });
        }
        Ok(Self {
            rpc_url: raw.rpc_url,
            keypair_path: PathBuf::from(raw.keypair_path),
            program_id: pk(&raw.program_id, "program_id")?,
            usdc_mint: pk(&raw.usdc_mint, "usdc_mint")?,
            kamino_reserve: pk_opt(&raw.kamino_reserve, "kamino_reserve")?,
            treasury_shares: match &raw.treasury_shares {
                Some(s) => Some(pk(s, "treasury_shares")?),
                None => None,
            },
            hourly_interval_secs: raw.hourly_interval_secs,
            fast_interval_secs: raw.fast_interval_secs,
            settle_interval_secs: raw.settle_interval_secs,
            market_open: MarketOpenMode::parse(&raw.market_open)?,
            alert_webhook_url: raw.alert_webhook_url,
            min_keeper_sol: raw.min_keeper_sol,
            lease_path: PathBuf::from(raw.lease_path),
            lease_ttl_secs: raw.lease_ttl_secs,
            status_bind: raw.status_bind,
            history_path: raw.history_path.map(PathBuf::from),
            feed: match raw.feed.as_str() {
                "onchain" => Feed::Onchain,
                "live" => Feed::Live,
                "mock" => Feed::Mock,
                other => return Err(anyhow!("feed must be onchain, live or mock (got {other})")),
            },
            phoenix_api_url: raw.phoenix_api_url.trim_end_matches('/').to_string(),
            kamino_api_url: raw.kamino_api_url.trim_end_matches('/').to_string(),
            kamino_market: raw.kamino_market,
            jupiter_price_url: raw.jupiter_price_url,
            mock_path: raw.mock_path.map(PathBuf::from),
            vaults,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn market_open_mode_parses() {
        assert_eq!(MarketOpenMode::parse("auto").unwrap(), MarketOpenMode::Auto);
        assert_eq!(MarketOpenMode::parse(" Open ").unwrap(), MarketOpenMode::Open);
        assert_eq!(MarketOpenMode::parse("closed").unwrap(), MarketOpenMode::Closed);
        assert!(MarketOpenMode::parse("maybe").is_err());
    }

    #[test]
    fn settle_and_market_open_defaults() {
        let raw: RawConfig = toml::from_str(r#"
rpc_url = "http://x"
keypair_path = "k"
program_id = "11111111111111111111111111111111"
usdc_mint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
[vaults.TSLA]
mint = "XsDoVfqeBukxuZHWhdvWHBhgEHjGNst4MLodqsJHzoB"
"#).unwrap();
        assert_eq!(raw.settle_interval_secs, 300);
        assert_eq!(raw.market_open, "auto");
        let cfg = Config::from_raw(raw).unwrap();
        assert_eq!(cfg.market_open, MarketOpenMode::Auto);
        assert_eq!(cfg.settle_interval_secs, 300);
    }
}
