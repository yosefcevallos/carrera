//! HTTP status server for the ops dashboard. Every vault is presented as a "book"
//! with its legs, net delta, estimated carry, rule inputs and health. Numbers come
//! from what the loops already read; nothing here touches RPC on request.
//!
//! Endpoints: `GET /status`, `GET /history?vault=TSLA&hours=168`, `GET /healthz`.

use crate::{
    accounts::{OverlayVault, VaultState},
    feed::FeedView,
    rule::{self, Decision, Inputs},
    Ctx,
};
use axum::{
    extract::{Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    io::Write,
    path::PathBuf,
    sync::Arc,
};

pub const HISTORY_CAP: usize = 14 * 24 * 60; // 14 days at one sample per minute
const ALERT_CAP: usize = 200;
const HEALTHZ_MAX_AGE_SECS: i64 = 180;

/// Phoenix liquidation buffer per tier (spec §6.1, "rise to Phoenix liq"), in bps.
pub fn phoenix_liq_buffer_bps(tier: u8) -> u32 {
    match tier {
        0 | 1 => 2600,
        2 => 2100,
        _ => 1600,
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Leg {
    pub kind: &'static str,
    pub venue: &'static str,
    pub size: u64,
    pub mark_e6: u64,
    pub rate_bps: i64,
    pub liq_price_e6: Option<u64>,
    pub liq_distance_bps: Option<i64>,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct NetDelta {
    pub qty: i64,
    pub usd_e6: i64,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Carry {
    pub accrued_usdc_e6: i64,
    pub ann_net_bps: i64,
    pub estimated: bool,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct RuleView {
    pub f_avg_bps: i64,
    pub parked_apy_bps: u32,
    pub r_bps: u32,
    pub hurdle_bps: i64,
    pub enter_bps: i64,
    pub exit_bps: i64,
    pub decision: String,
    pub samples: u8,
}

#[derive(Serialize, Clone, Debug)]
pub struct VaultBook {
    pub symbol: String,
    pub state: String,
    pub step: u8,
    pub market_open: bool,
    pub opened_ts: i64,
    pub tier: u8,
    pub stock_decimals: u8,
    pub usdc_decimals: u8,
    /// Spot mark from the vault's cached oracle price.
    pub spot_mark_e6: u64,
    /// Perp mark. Until Hawkeye is wired this is the same cached price as `spot_mark_e6`.
    pub perp_mark_e6: u64,
    /// Basis only: Phoenix equity above `min_margin × short notional`. None outside Basis.
    pub idle_margin_usdc_e6: Option<i64>,
    /// Basis only: `(perp_mark − spot_mark) / spot_mark` in bps at the moment Basis was entered.
    pub basis_at_open_bps: Option<i64>,
    /// True when both marks used for `basis_at_open_bps` came from the same cached price.
    pub basis_estimated: bool,
    pub legs: Vec<Leg>,
    pub net_delta: NetDelta,
    pub carry: Carry,
    pub rule: RuleView,
    pub ltv_bps: u32,
    pub liq_ltv_bps: u32,
    pub margin_bps: Option<u32>,
    pub min_margin_bps: u32,
    pub emergency_ltv_bps: u32,
    pub nav_usd_e6: u64,
    pub share_price_stock_e6: u64,
    pub nav_slot: u64,
    pub nav_age_slots: u64,
    pub pending_exit_shares: u64,
    pub epoch_id: u64,
    /// Live-feed inputs the keeper last fetched for this vault (None under onchain / mock).
    pub feed: Option<FeedView>,
}

#[derive(Serialize, Clone, Debug)]
pub struct AlertRecord {
    pub level: &'static str,
    pub vault: Option<String>,
    pub message: String,
    pub ts: i64,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct KeeperView {
    pub instance_id: String,
    pub is_leader: bool,
    pub last_hourly_run_ts: i64,
    pub last_fast_run_ts: i64,
    pub hourly_ok: bool,
    pub fast_ok: bool,
    pub sol_balance: f64,
    pub program_id: String,
    pub cluster: String,
    pub registry_paused: bool,
    pub alerts: Vec<AlertRecord>,
}

#[derive(Serialize)]
pub struct StatusResponse<'a> {
    pub keeper: &'a KeeperView,
    pub vaults: Vec<&'a VaultBook>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct HistoryPoint {
    pub ts: i64,
    pub state: String,
    pub f_avg_bps: i64,
    pub hurdle_bps: i64,
    pub ltv_bps: u32,
    pub margin_bps: Option<u32>,
    pub nav_usd_e6: u64,
    pub share_price_stock_e6: u64,
}

/// Per-vault accrual state, reset whenever the on-chain state changes.
#[derive(Clone, Debug)]
pub struct CarryTracker {
    pub state: u8,
    pub entered_ts: i64,
    pub last_ts: i64,
    pub accrued_usdc_e6: i64,
    /// Perp-vs-spot basis in bps recorded when the vault entered Basis.
    pub basis_at_open_bps: Option<i64>,
}

/// What the loops write and the handlers read.
#[derive(Default)]
pub struct StatusState {
    pub keeper: KeeperView,
    pub books: BTreeMap<String, VaultBook>,
    pub history: HashMap<String, VecDeque<HistoryPoint>>,
    pub trackers: HashMap<String, CarryTracker>,
    pub history_path: Option<PathBuf>,
}

pub fn now_ts() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Rates the book needs beyond the vault account.
#[derive(Clone, Copy, Debug, Default)]
pub struct Rates {
    pub borrow_bps: u32,
    pub supply_bps: u32,
    pub paused: bool,
}

/// Liquidation price of the Kamino collateral: LTV reaches `liq_ltv` when the price
/// falls to `price × ltv / liq_ltv`. Distance is `(1 − ltv / liq_ltv)` in bps.
pub fn kamino_liq(price_e6: u64, ltv_bps: u32, liq_ltv_bps: u32) -> (Option<u64>, Option<i64>) {
    if ltv_bps == 0 || liq_ltv_bps == 0 {
        return (None, None);
    }
    let liq = (price_e6 as u128 * ltv_bps as u128 / liq_ltv_bps as u128) as u64;
    let dist = 10_000 - (ltv_bps as i64 * 10_000 / liq_ltv_bps as i64);
    (Some(liq), Some(dist))
}

/// Phoenix short liquidation estimate: `mark × (1 + tier buffer)`, spec §6.1.
pub fn phoenix_liq(mark_e6: u64, tier: u8) -> (Option<u64>, Option<i64>) {
    let b = phoenix_liq_buffer_bps(tier);
    (Some((mark_e6 as u128 * (10_000 + b as u128) / 10_000) as u64), Some(b as i64))
}

pub fn legs(v: &OverlayVault, f_avg_bps: i64, rates: Rates, stock_decimals: u8) -> Vec<Leg> {
    let mut out = Vec::new();
    let p = &v.params;
    let ltv = v.ltv_bps(stock_decimals);
    let (k_liq, k_dist) = kamino_liq(v.price_e6, ltv, p.liq_ltv_bps);
    if v.basis_spot_qty > 0 {
        out.push(Leg { kind: "long_spot", venue: "Kamino", size: v.basis_spot_qty, mark_e6: v.price_e6, rate_bps: 0, liq_price_e6: k_liq, liq_distance_bps: k_dist });
    }
    let debt = v.debt_usdc.saturating_add(v.debt_b_usdc);
    if debt > 0 {
        out.push(Leg { kind: "borrow_usdc", venue: "Kamino", size: debt, mark_e6: 1_000_000, rate_bps: -(rates.borrow_bps as i64), liq_price_e6: k_liq, liq_distance_bps: k_dist });
    }
    if v.parked_usdc > 0 {
        out.push(Leg { kind: "supply_usdc", venue: "Kamino", size: v.parked_usdc, mark_e6: 1_000_000, rate_bps: rates.supply_bps as i64, liq_price_e6: None, liq_distance_bps: None });
    }
    if v.phoenix_short_qty > 0 {
        let (liq, dist) = phoenix_liq(v.price_e6, v.tier);
        out.push(Leg { kind: "short_perp", venue: "Phoenix", size: v.phoenix_short_qty, mark_e6: v.price_e6, rate_bps: f_avg_bps, liq_price_e6: liq, liq_distance_bps: dist });
    }
    out
}

/// Perp mark vs spot mark in bps: `(perp − spot) / spot`. None when spot is zero.
pub fn basis_bps(perp_mark_e6: u64, spot_mark_e6: u64) -> Option<i64> {
    if spot_mark_e6 == 0 {
        return None;
    }
    Some(((perp_mark_e6 as i128 - spot_mark_e6 as i128) * 10_000 / spot_mark_e6 as i128) as i64)
}

/// Phoenix free collateral above the maintenance floor: `equity − min_margin × short notional`.
/// Negative means the subaccount is below `min_margin`.
pub fn idle_margin_usdc_e6(v: &OverlayVault, perp_mark_e6: u64, stock_decimals: u8) -> i64 {
    let notional = v.phoenix_short_qty as i128 * perp_mark_e6 as i128 / 10i128.pow(stock_decimals as u32);
    let required = notional * v.params.min_margin_bps as i128 / 10_000;
    (v.phoenix_equity_usdc as i128 - required) as i64
}

/// Basis legs only: depositor stock is excluded.
pub fn net_delta(v: &OverlayVault, stock_decimals: u8) -> NetDelta {
    let qty = v.basis_spot_qty as i64 - v.phoenix_short_qty as i64;
    let usd = qty as i128 * v.price_e6 as i128 / 10i128.pow(stock_decimals as u32);
    NetDelta { qty, usd_e6: usd as i64 }
}

/// Annualised net carry on basis notional: `(f_avg | s) − r − L·r`.
pub fn ann_net_bps(state: VaultState, f_avg_bps: i64, v: &OverlayVault, rates: Rates) -> i64 {
    let gross = match state {
        VaultState::Basis | VaultState::Winding | VaultState::Unwinding => f_avg_bps,
        VaultState::Parked => rates.supply_bps as i64,
        VaultState::Idle => return 0,
    };
    let l_r = v.params.ltv_bps as i64 * rates.borrow_bps as i64 / 10_000;
    gross - rates.borrow_bps as i64 - l_r
}

/// USDC accrued over `dt_secs`: funding on the short notional plus supply interest
/// minus borrow interest, all annualised rates in bps.
pub fn carry_increment_e6(v: &OverlayVault, f_avg_bps: i64, rates: Rates, stock_decimals: u8, dt_secs: i64) -> i64 {
    if dt_secs <= 0 {
        return 0;
    }
    let short_notional = v.phoenix_short_qty as i128 * v.price_e6 as i128 / 10i128.pow(stock_decimals as u32);
    let debt = v.debt_usdc as i128 + v.debt_b_usdc as i128;
    let per_year = f_avg_bps as i128 * short_notional + rates.supply_bps as i128 * v.parked_usdc as i128 - rates.borrow_bps as i128 * debt;
    (per_year * dt_secs as i128 / (10_000 * 365 * 86_400)) as i64
}

impl StatusState {
    pub fn alert(&mut self, level: &'static str, vault: Option<&str>, message: String) {
        self.keeper.alerts.push(AlertRecord { level, vault: vault.map(str::to_string), message, ts: now_ts() });
        if self.keeper.alerts.len() > ALERT_CAP {
            let drop = self.keeper.alerts.len() - ALERT_CAP;
            self.keeper.alerts.drain(..drop);
        }
    }

    /// Attach (or clear) the live-feed view for a vault's book.
    pub fn set_feed(&mut self, symbol: &str, feed: Option<FeedView>) {
        if let Some(b) = self.books.get_mut(symbol) {
            b.feed = feed;
        }
    }

    /// Rebuild one vault's book from a fresh account read. Called once per fast tick.
    /// `stock_decimals` is the config fallback used when the vault account reports 0.
    pub fn record_vault(&mut self, symbol: &str, v: &OverlayVault, stock_decimals: u8, rates: Rates, current_slot: u64) {
        let now = now_ts();
        let state = v.state().unwrap_or(VaultState::Idle);
        let f_avg = v.f_avg_bps();
        let stock_decimals = if v.stock_decimals > 0 { v.stock_decimals } else { stock_decimals };
        // Both marks are the vault's cached price until Hawkeye is wired (spec §7.1).
        let spot_mark_e6 = v.price_e6;
        let perp_mark_e6 = v.price_e6;
        let basis_estimated = true;

        // Recorded on the tick the keeper first sees the vault in Basis (a transition, or first sight).
        let basis_now = if state == VaultState::Basis { basis_bps(perp_mark_e6, spot_mark_e6) } else { None };
        let tracker = self.trackers.entry(symbol.to_string()).or_insert(CarryTracker { state: v.state, entered_ts: now, last_ts: now, accrued_usdc_e6: 0, basis_at_open_bps: basis_now });
        if tracker.state != v.state {
            *tracker = CarryTracker { state: v.state, entered_ts: now, last_ts: now, accrued_usdc_e6: 0, basis_at_open_bps: basis_now };
        } else {
            tracker.accrued_usdc_e6 += carry_increment_e6(v, f_avg, rates, stock_decimals, now - tracker.last_ts);
            tracker.last_ts = now;
        }
        let opened_ts = tracker.entered_ts;
        let accrued = tracker.accrued_usdc_e6;
        let basis_at_open_bps = tracker.basis_at_open_bps;
        let idle_margin = if state == VaultState::Basis { Some(idle_margin_usdc_e6(v, perp_mark_e6, stock_decimals)) } else { None };

        let h = rule::hurdles(&v.params, rates.supply_bps, rates.borrow_bps);
        let hurdle = if state == VaultState::Idle { h.from_idle_bps } else { h.from_parked_bps };
        let decision = rule::evaluate(
            &v.params,
            &Inputs { state, f_avg_bps: f_avg, samples: v.funding_samples, supply_bps: rates.supply_bps, borrow_bps: rates.borrow_bps, market_open: v.market_open, paused: rates.paused },
        );
        let ltv = v.ltv_bps(stock_decimals);
        let margin = v.margin_bps(stock_decimals);
        let book = VaultBook {
            symbol: symbol.to_string(),
            state: state.name().to_lowercase(),
            step: v.step,
            market_open: v.market_open,
            opened_ts,
            tier: v.tier,
            stock_decimals,
            usdc_decimals: 6,
            spot_mark_e6,
            perp_mark_e6,
            idle_margin_usdc_e6: idle_margin,
            basis_at_open_bps,
            basis_estimated,
            legs: legs(v, f_avg, rates, stock_decimals),
            net_delta: net_delta(v, stock_decimals),
            carry: Carry { accrued_usdc_e6: accrued, ann_net_bps: ann_net_bps(state, f_avg, v, rates), estimated: true },
            rule: RuleView {
                f_avg_bps: f_avg,
                parked_apy_bps: rates.supply_bps,
                r_bps: rates.borrow_bps,
                hurdle_bps: hurdle,
                enter_bps: hurdle + v.params.enter_margin_bps as i64,
                exit_bps: hurdle - v.params.exit_margin_bps as i64,
                decision: decision_name(decision).into(),
                samples: v.funding_samples,
            },
            ltv_bps: ltv,
            liq_ltv_bps: v.params.liq_ltv_bps,
            margin_bps: margin,
            min_margin_bps: v.params.min_margin_bps,
            emergency_ltv_bps: v.params.emergency_ltv_bps,
            nav_usd_e6: v.nav_usd_e6,
            share_price_stock_e6: v.share_price_stock_e6,
            nav_slot: v.nav_slot,
            nav_age_slots: current_slot.saturating_sub(v.nav_slot),
            pending_exit_shares: v.pending_exit_shares,
            epoch_id: v.epoch_id,
            feed: self.books.get(symbol).and_then(|b| b.feed.clone()),
        };
        let point = HistoryPoint {
            ts: now,
            state: book.state.clone(),
            f_avg_bps: f_avg,
            hurdle_bps: hurdle,
            ltv_bps: ltv,
            margin_bps: margin,
            nav_usd_e6: v.nav_usd_e6,
            share_price_stock_e6: v.share_price_stock_e6,
        };
        self.books.insert(symbol.to_string(), book);
        let ring = self.history.entry(symbol.to_string()).or_default();
        ring.push_back(point.clone());
        while ring.len() > HISTORY_CAP {
            ring.pop_front();
        }
        if let Some(path) = &self.history_path {
            if let Err(e) = append_jsonl(path, symbol, &point) {
                tracing::warn!("history append failed: {e:#}");
            }
        }
    }
}

fn decision_name(d: Decision) -> &'static str {
    match d {
        Decision::None => "none",
        Decision::ToBasis => "to_basis",
        Decision::ToParked => "to_parked",
        Decision::ToIdle => "to_idle",
    }
}

fn append_jsonl(path: &PathBuf, symbol: &str, p: &HistoryPoint) -> anyhow::Result<()> {
    #[derive(Serialize)]
    struct Line<'a> {
        vault: &'a str,
        #[serde(flatten)]
        point: &'a HistoryPoint,
    }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    serde_json::to_writer(&mut f, &Line { vault: symbol, point: p })?;
    f.write_all(b"\n")?;
    Ok(())
}

// ---- HTTP -----------------------------------------------------------------

fn cors(mut r: Response) -> Response {
    r.headers_mut().insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    r
}

async fn status_handler(State(ctx): State<Arc<Ctx>>) -> Response {
    let s = ctx.status.read().await;
    let body = StatusResponse { keeper: &s.keeper, vaults: s.books.values().collect() };
    cors(Json(&body).into_response())
}

#[derive(Deserialize)]
struct HistoryQuery {
    vault: String,
    #[serde(default = "default_hours")]
    hours: i64,
}
fn default_hours() -> i64 {
    168
}

async fn history_handler(State(ctx): State<Arc<Ctx>>, Query(q): Query<HistoryQuery>) -> Response {
    let s = ctx.status.read().await;
    let since = now_ts() - q.hours.max(0) * 3600;
    let points: Vec<&HistoryPoint> = s.history.get(&q.vault).map(|r| r.iter().filter(|p| p.ts >= since).collect()).unwrap_or_default();
    cors(Json(&points).into_response())
}

async fn healthz_handler(State(ctx): State<Arc<Ctx>>) -> Response {
    let s = ctx.status.read().await;
    let fresh = now_ts() - s.keeper.last_fast_run_ts <= HEALTHZ_MAX_AGE_SECS;
    let code = if fresh { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    cors((code, if fresh { "ok" } else { "fast loop stale" }).into_response())
}

pub fn router(ctx: Arc<Ctx>) -> Router {
    Router::new()
        .route("/status", get(status_handler))
        .route("/history", get(history_handler))
        .route("/healthz", get(healthz_handler))
        .with_state(ctx)
}

pub async fn serve(ctx: Arc<Ctx>) -> anyhow::Result<()> {
    let bind = ctx.cfg.status_bind.clone();
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!("status server on http://{bind}");
    axum::serve(listener, router(ctx)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::VaultParams;

    fn basis_vault() -> OverlayVault {
        OverlayVault {
            tier: 1,
            state: 3,
            params: VaultParams { ltv_bps: 3000, liq_ltv_bps: 6500, min_margin_bps: 1200, emergency_ltv_bps: 6000, enter_margin_bps: 200, exit_margin_bps: 100, carry_guard_margin_bps: 50, expected_hold_hours: 720, roundtrip_cost_bps: 60, funding_window: 24, ..Default::default() },
            collateral_qty: 130 * 10u64.pow(8),
            basis_spot_qty: 30 * 10u64.pow(8),
            debt_usdc: 12_000_000_000,
            debt_b_usdc: 3_600_000_000,
            phoenix_short_qty: 30 * 10u64.pow(8),
            phoenix_equity_usdc: 1_440_000_000,
            price_e6: 400_000_000,
            funding: [399_543; 24],
            funding_samples: 24,
            ..Default::default()
        }
    }

    const RATES: Rates = Rates { borrow_bps: 590, supply_bps: 480, paused: false };

    #[test]
    fn legs_and_liq_prices() {
        let v = basis_vault();
        let l = legs(&v, v.f_avg_bps(), RATES, 8);
        let kinds: Vec<_> = l.iter().map(|x| x.kind).collect();
        assert_eq!(kinds, ["long_spot", "borrow_usdc", "short_perp"]);
        assert_eq!(l[1].size, 15_600_000_000);
        assert_eq!(l[1].rate_bps, -590);
        assert_eq!(l[2].rate_bps, 3499);
        // Tier B: short liq at mark × 1.26.
        assert_eq!(l[2].liq_price_e6, Some(504_000_000));
        assert_eq!(l[2].liq_distance_bps, Some(2600));
        // Kamino: ltv = 15600 / 52000 = 30%; liq at 65% → price × 0.3 / 0.65.
        let (liq, dist) = kamino_liq(400_000_000, 3000, 6500);
        assert_eq!(liq, Some(184_615_384));
        assert_eq!(dist, Some(5385));
        assert_eq!(l[0].liq_price_e6, liq);
        assert_eq!(kamino_liq(400_000_000, 0, 6500), (None, None));
    }

    #[test]
    fn net_delta_and_carry_helpers() {
        let mut v = basis_vault();
        assert_eq!(net_delta(&v, 8), NetDelta { qty: 0, usd_e6: 0 });
        v.phoenix_short_qty = 29 * 10u64.pow(8);
        assert_eq!(net_delta(&v, 8), NetDelta { qty: 100_000_000, usd_e6: 400_000_000 });
        // ann_net = f_avg − r − L·r = 3499 − 590 − 177.
        assert_eq!(ann_net_bps(VaultState::Basis, 3499, &v, RATES), 2732);
        assert_eq!(ann_net_bps(VaultState::Parked, 3499, &v, RATES), 480 - 590 - 177);
        assert_eq!(ann_net_bps(VaultState::Idle, 3499, &v, RATES), 0);
        // One year of carry on the full basis book: funding 34.99% × $12,000 − 5.9% × $15,600.
        let v = basis_vault();
        let year = carry_increment_e6(&v, 3499, RATES, 8, 365 * 86_400);
        let expected = (3499i128 * 12_000_000_000 - 590i128 * 15_600_000_000) / 10_000;
        assert_eq!(year as i128, expected);
        assert_eq!(carry_increment_e6(&v, 3499, RATES, 8, 0), 0);
    }

    #[test]
    fn basis_and_idle_margin_helpers() {
        assert_eq!(basis_bps(401_000_000, 400_000_000), Some(25));
        assert_eq!(basis_bps(399_000_000, 400_000_000), Some(-25));
        assert_eq!(basis_bps(1, 0), None);
        let v = basis_vault();
        // equity $1,440 − 12% × $12,000 = $0 idle.
        assert_eq!(idle_margin_usdc_e6(&v, 400_000_000, 8), 0);
        let mut rich = v.clone();
        rich.phoenix_equity_usdc = 2_000_000_000;
        assert_eq!(idle_margin_usdc_e6(&rich, 400_000_000, 8), 560_000_000);
        let mut poor = v.clone();
        poor.phoenix_equity_usdc = 1_000_000_000;
        assert_eq!(idle_margin_usdc_e6(&poor, 400_000_000, 8), -440_000_000);
    }

    #[test]
    fn book_serialises_and_history_ring_caps() {
        let mut s = StatusState::default();
        let v = basis_vault();
        s.record_vault("TSLA", &v, 8, RATES, 1_000);
        let json = serde_json::to_value(StatusResponse { keeper: &s.keeper, vaults: s.books.values().collect() }).unwrap();
        let b = &json["vaults"][0];
        assert_eq!(b["symbol"], "TSLA");
        assert_eq!(b["state"], "basis");
        assert_eq!(b["carry"]["estimated"], true);
        assert_eq!(b["rule"]["hurdle_bps"], 480 + 177 + 730);
        assert_eq!(b["rule"]["enter_bps"], 1387 + 200);
        assert_eq!(b["legs"].as_array().unwrap().len(), 3);
        assert_eq!(b["ltv_bps"], 3000);
        assert_eq!(b["margin_bps"], 1200);
        assert_eq!(b["stock_decimals"], 8);
        assert_eq!(b["usdc_decimals"], 6);
        assert_eq!(b["spot_mark_e6"], 400_000_000);
        assert_eq!(b["perp_mark_e6"], 400_000_000);
        assert_eq!(b["idle_margin_usdc_e6"], 0);
        assert_eq!(b["basis_at_open_bps"], 0);
        assert_eq!(b["basis_estimated"], true);
        assert_eq!(json["keeper"]["registry_paused"], false);
        assert!(json["keeper"]["alerts"].as_array().unwrap().is_empty());
        // Vault-reported decimals win over the config fallback.
        let mut six = v.clone();
        six.stock_decimals = 6;
        s.record_vault("SIX", &six, 8, RATES, 1_000);
        assert_eq!(s.books["SIX"].stock_decimals, 6);

        // State change resets the carry tracker.
        let mut parked = v.clone();
        parked.state = 1;
        s.record_vault("TSLA", &parked, 8, RATES, 1_010);
        assert_eq!(s.books["TSLA"].carry.accrued_usdc_e6, 0);
        assert_eq!(s.books["TSLA"].state, "parked");
        assert_eq!(s.books["TSLA"].idle_margin_usdc_e6, None);
        assert_eq!(s.books["TSLA"].basis_at_open_bps, None);

        for _ in 0..(HISTORY_CAP + 5) {
            s.record_vault("TSLA", &parked, 8, RATES, 1_010);
        }
        assert_eq!(s.history["TSLA"].len(), HISTORY_CAP);
        let hp: HistoryPoint = serde_json::from_value(serde_json::to_value(s.history["TSLA"].back().unwrap()).unwrap()).unwrap();
        assert_eq!(hp.state, "parked");

        s.alert("warn", None, "x".into());
        assert_eq!(s.keeper.alerts.len(), 1);
    }
}
