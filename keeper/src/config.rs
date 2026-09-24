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
    /// symbol -> vault. Nine entries in production.
    pub vaults: BTreeMap<String, RawVault>,
}

fn default_hourly() -> u64 {
    3600
}
fn default_fast() -> u64 {
    60
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
    pub alert_webhook_url: Option<String>,
    pub min_keeper_sol: f64,
    pub lease_path: PathBuf,
    pub lease_ttl_secs: u64,
    pub status_bind: String,
    pub history_path: Option<PathBuf>,
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
            alert_webhook_url: raw.alert_webhook_url,
            min_keeper_sol: raw.min_keeper_sol,
            lease_path: PathBuf::from(raw.lease_path),
            lease_ttl_secs: raw.lease_ttl_secs,
            status_bind: raw.status_bind,
            history_path: raw.history_path.map(PathBuf::from),
            vaults,
        })
    }
}
