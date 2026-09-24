//! Operator alerts (spec §12): stuck intermediate state, health near emergency,
//! stale NAV, low keeper SOL. Each condition logs at WARN and, if a webhook is
//! configured, POSTs `{"text": ...}`. The same alert key is not re-sent within
//! the cooldown.

use crate::{
    accounts::{OverlayVault, VaultState},
    status::{now_ts, AlertRecord},
};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

pub const STUCK_AFTER: Duration = Duration::from_secs(15 * 60);
pub const HEALTH_WARN_BAND_BPS: u32 = 300;
const COOLDOWN: Duration = Duration::from_secs(15 * 60);

pub struct Alerts {
    webhook: Option<String>,
    client: reqwest::Client,
    stuck_since: HashMap<String, Instant>,
    last_sent: HashMap<String, Instant>,
    /// Alerts raised since the last `drain`, for the status server.
    pending: Vec<AlertRecord>,
}

impl Alerts {
    pub fn new(webhook: Option<String>) -> Self {
        Self { webhook, client: reqwest::Client::new(), stuck_since: HashMap::new(), last_sent: HashMap::new(), pending: Vec::new() }
    }

    pub fn drain(&mut self) -> Vec<AlertRecord> {
        std::mem::take(&mut self.pending)
    }

    /// `level` is "warn" or "crit". `vault` is the symbol when the alert is vault-specific.
    pub async fn fire(&mut self, level: &'static str, vault: Option<&str>, key: &str, text: String) {
        let now = Instant::now();
        if let Some(t) = self.last_sent.get(key) {
            if now.duration_since(*t) < COOLDOWN {
                return;
            }
        }
        self.last_sent.insert(key.to_string(), now);
        self.pending.push(AlertRecord { level, vault: vault.map(str::to_string), message: text.clone(), ts: now_ts() });
        tracing::warn!(alert = key, level, "{text}");
        if let Some(url) = &self.webhook {
            let body = serde_json::json!({ "text": text });
            if let Err(e) = self.client.post(url).json(&body).send().await {
                tracing::error!("alert webhook failed: {e}");
            }
        }
    }

    /// Conditions that can be judged from one vault account read.
    pub async fn check_vault(&mut self, symbol: &str, v: &OverlayVault, stock_decimals: u8, current_slot: u64) {
        let state = match v.state() {
            Ok(s) => s,
            Err(e) => {
                self.fire("crit", Some(symbol), &format!("{symbol}:badstate"), format!("{symbol}: {e}")).await;
                return;
            }
        };

        // Stuck in Winding / Unwinding for more than 15 minutes.
        if matches!(state, VaultState::Winding | VaultState::Unwinding) {
            let since = *self.stuck_since.entry(symbol.to_string()).or_insert_with(Instant::now);
            if since.elapsed() > STUCK_AFTER {
                self.fire(
                    "crit",
                    Some(symbol),
                    &format!("{symbol}:stuck"),
                    format!("{symbol}: stuck in {} step {} for {}m", state.name(), v.step, since.elapsed().as_secs() / 60),
                )
                .await;
            }
        } else {
            self.stuck_since.remove(symbol);
        }

        // Health within 300 bps of an emergency trigger.
        let p = &v.params;
        if state != VaultState::Idle {
            let ltv = v.ltv_bps(stock_decimals);
            if ltv + HEALTH_WARN_BAND_BPS >= p.emergency_ltv_bps && p.emergency_ltv_bps > 0 {
                self.fire("crit", Some(symbol), &format!("{symbol}:ltv"), format!("{symbol}: LTV {ltv} bps within 300 bps of emergency {}", p.emergency_ltv_bps)).await;
            }
        }
        if state == VaultState::Basis {
            if let Some(m) = v.margin_bps(stock_decimals) {
                if m <= p.min_margin_bps + HEALTH_WARN_BAND_BPS {
                    self.fire("crit", Some(symbol), &format!("{symbol}:margin"), format!("{symbol}: Phoenix margin {m} bps within 300 bps of emergency {}", p.min_margin_bps)).await;
                }
            }
        }

        // NAV cache older than the program will accept for deposits and settlement.
        if v.nav_stale(current_slot) {
            self.fire("warn", Some(symbol), &format!("{symbol}:nav"), format!("{symbol}: NAV stale ({} slots old, max {})", current_slot.saturating_sub(v.nav_slot), p.max_nav_age_slots)).await;
        }
    }

    pub async fn check_sol(&mut self, balance_sol: f64, min_sol: f64) {
        if balance_sol < min_sol {
            self.fire("warn", None, "keeper:sol", format!("keeper SOL {balance_sol:.3} below {min_sol}")).await;
        }
    }
}
