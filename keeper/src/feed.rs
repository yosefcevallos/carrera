//! Live public-API feed for the inputs the program cannot yet read itself
//! (`mock-venues` build): Phoenix hourly funding, Kamino USDC borrow/supply APY,
//! xStock USD prices from Jupiter, and Phoenix's per-market session state.
//!
//! Endpoints (all public, no auth):
//! - `GET {phoenix}/v1/funding/{symbol}/rates?limit=N`  → `{ marketId, symbol, rates: [{ timestamp (unix s), fundingRatePercentage }] }`
//! - `GET {phoenix}/v1/view/exchange/markets`            → `[{ symbol, marketStatus, commodityMetadata: { status, isAfterHours }, metadata: { calendar } }]`
//! - `GET {phoenix}/v1/market/{symbol}/market-calendar`   → `{ calendar: { weeklySchedule: { Mon: { sessions: [{ start, end, mode }] } }, dateOverrides: { "YYYY-MM-DD": {...} } } }`
//! - `GET {kamino}/kamino-market/{market}/reserves/metrics` → `[{ liquidityToken, borrowApy, supplyApy, totalSupply, totalSupplyUsd, ... }]`
//! - `GET {jupiter}?ids=mint1,mint2`                       → `{ "<mint>": { usdPrice, decimals, ... } }`
//!
//! Units: Phoenix `fundingRatePercentage` is the percentage paid per hourly interval,
//! positive when longs pay shorts. The program stores hourly bps × 1e6, so
//! `scaled = pct × 100 × 1e6`. Kamino APYs are decimals (0.0589 = 5.89%) → bps × 1e4.
//! Jupiter `usdPrice` is a float → `price_e6 = round(usd × 1e6)`.

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Datelike, NaiveTime, Timelike, Utc};
use chrono_tz::America::New_York;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Program funding unit: hourly rate in bps × 1e6.
pub const FUNDING_SCALE: i64 = 1_000_000;

// ---------------------------------------------------------------- conversions

/// `"0.004209"` (percent per hour) → `420_900` (bps × 1e6). Rounded to nearest.
pub fn funding_pct_to_scaled(pct: &str) -> Option<i64> {
    let p: f64 = pct.trim().parse().ok()?;
    if !p.is_finite() {
        return None;
    }
    Some((p * 100.0 * FUNDING_SCALE as f64).round() as i64)
}

/// `"0.0589"` → `589` bps.
pub fn apy_decimal_to_bps(apy: &str) -> Option<u32> {
    let a: f64 = apy.trim().parse().ok()?;
    if !a.is_finite() || a < 0.0 {
        return None;
    }
    Some((a * 10_000.0).round() as u32)
}

pub fn usd_to_e6(usd: f64) -> Option<u64> {
    if !usd.is_finite() || usd <= 0.0 {
        return None;
    }
    Some((usd * 1e6).round() as u64)
}

// ---------------------------------------------------------------- Phoenix funding

#[derive(Deserialize)]
struct FundingRates {
    rates: Vec<FundingPoint>,
}

#[derive(Deserialize)]
struct FundingPoint {
    timestamp: i64,
    #[serde(rename = "fundingRatePercentage")]
    pct: String,
}

/// Latest funding sample: `(unix_ts, scaled)`.
pub fn parse_funding_latest(json: &str) -> Result<Option<(i64, i64)>> {
    let r: FundingRates = serde_json::from_str(json).context("phoenix funding json")?;
    let last = r.rates.iter().max_by_key(|p| p.timestamp);
    Ok(last.and_then(|p| funding_pct_to_scaled(&p.pct).map(|s| (p.timestamp, s))))
}

// ---------------------------------------------------------------- Phoenix markets

#[derive(Deserialize)]
struct Market {
    symbol: String,
    #[serde(rename = "marketStatus")]
    market_status: String,
    #[serde(rename = "commodityMetadata")]
    commodity: Option<Commodity>,
}

#[derive(Deserialize)]
struct Commodity {
    status: Option<String>,
    #[serde(rename = "isAfterHours")]
    is_after_hours: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MarketInfo {
    pub active: bool,
    pub after_hours: bool,
}

pub fn parse_markets(json: &str) -> Result<BTreeMap<String, MarketInfo>> {
    let ms: Vec<Market> = serde_json::from_str(json).context("phoenix markets json")?;
    Ok(ms
        .into_iter()
        .map(|m| {
            let c = m.commodity.as_ref();
            let active = m.market_status == "active" && c.and_then(|c| c.status.as_deref()).map(|s| s == "active").unwrap_or(true);
            let after_hours = c.and_then(|c| c.is_after_hours).unwrap_or(false);
            (m.symbol, MarketInfo { active, after_hours })
        })
        .collect())
}

// ---------------------------------------------------------------- Phoenix calendar

#[derive(Deserialize, Clone)]
pub struct CalendarDoc {
    calendar: Calendar,
}

#[derive(Deserialize, Clone)]
struct Calendar {
    #[serde(rename = "weeklySchedule", default)]
    weekly: BTreeMap<String, Day>,
    #[serde(rename = "dateOverrides", default)]
    overrides: BTreeMap<String, Day>,
}

#[derive(Deserialize, Clone)]
struct Day {
    sessions: Vec<Session>,
}

#[derive(Deserialize, Clone)]
struct Session {
    start: String,
    end: String,
    mode: String,
}

pub fn parse_calendar(json: &str) -> Result<CalendarDoc> {
    serde_json::from_str(json).context("phoenix calendar json")
}

fn hms(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s, "%H:%M:%S").ok()
}

/// True when `now` falls inside a `CASH_SESSION` on the Phoenix calendar
/// (New York local time; a date override replaces that day's weekly schedule).
pub fn calendar_cash_open(cal: &CalendarDoc, now: DateTime<Utc>) -> bool {
    let ny = now.with_timezone(&New_York);
    let date = ny.format("%Y-%m-%d").to_string();
    let wd = ny.weekday().to_string(); // "Mon".."Sun"
    let day = cal.calendar.overrides.get(&date).or_else(|| cal.calendar.weekly.get(&wd));
    let Some(day) = day else { return false };
    let t = NaiveTime::from_hms_opt(ny.hour(), ny.minute(), ny.second()).expect("valid time");
    day.sessions.iter().any(|s| {
        if s.mode != "CASH_SESSION" {
            return false;
        }
        let (Some(start), Some(end)) = (hms(&s.start), hms(&s.end)) else { return false };
        let end_is_midnight = end == NaiveTime::from_hms_opt(0, 0, 0).unwrap();
        t >= start && (end_is_midnight || t < end)
    })
}

// ---------------------------------------------------------------- Kamino

#[derive(Deserialize)]
struct Reserve {
    #[serde(rename = "liquidityToken")]
    token: String,
    #[serde(rename = "liquidityTokenMint")]
    mint: String,
    #[serde(rename = "borrowApy")]
    borrow: String,
    #[serde(rename = "supplyApy")]
    supply: String,
    #[serde(rename = "totalSupply")]
    total_supply: String,
    #[serde(rename = "totalSupplyUsd")]
    total_supply_usd: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KaminoRates {
    pub borrow_bps: u32,
    pub supply_bps: u32,
}

/// Borrow/supply APY of the reserve whose `liquidityToken` is `token` (e.g. "USDC").
pub fn parse_kamino_rates(json: &str, token: &str) -> Result<KaminoRates> {
    let rs: Vec<Reserve> = serde_json::from_str(json).context("kamino metrics json")?;
    let r = rs.iter().find(|r| r.token == token).ok_or_else(|| anyhow!("kamino: no {token} reserve"))?;
    Ok(KaminoRates {
        borrow_bps: apy_decimal_to_bps(&r.borrow).ok_or_else(|| anyhow!("kamino: bad borrowApy"))?,
        supply_bps: apy_decimal_to_bps(&r.supply).ok_or_else(|| anyhow!("kamino: bad supplyApy"))?,
    })
}

/// Implied USD price per reserve mint (`totalSupplyUsd / totalSupply`); fallback when Jupiter is down.
pub fn parse_kamino_prices(json: &str) -> Result<BTreeMap<String, u64>> {
    let rs: Vec<Reserve> = serde_json::from_str(json).context("kamino metrics json")?;
    Ok(rs
        .iter()
        .filter_map(|r| {
            let s: f64 = r.total_supply.parse().ok()?;
            let u: f64 = r.total_supply_usd.parse().ok()?;
            if s <= 0.0 {
                return None;
            }
            Some((r.mint.clone(), usd_to_e6(u / s)?))
        })
        .collect())
}

// ---------------------------------------------------------------- Jupiter

#[derive(Deserialize)]
struct JupPrice {
    #[serde(rename = "usdPrice")]
    usd: f64,
}

/// mint → price_e6.
pub fn parse_jupiter_prices(json: &str) -> Result<BTreeMap<String, u64>> {
    let m: BTreeMap<String, Option<JupPrice>> = serde_json::from_str(json).context("jupiter price json")?;
    Ok(m.into_iter().filter_map(|(k, v)| v.and_then(|p| usd_to_e6(p.usd)).map(|e6| (k, e6))).collect())
}

// ---------------------------------------------------------------- snapshot + fetcher

/// What `/status` shows per vault so operators can see where each number came from.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct FeedView {
    pub funding_hourly_scaled: Option<i64>,
    pub funding_ts: Option<i64>,
    pub borrow_bps: Option<u32>,
    pub supply_bps: Option<u32>,
    pub price_e6: Option<u64>,
    pub phoenix_open: Option<bool>,
    pub fetched_ts: i64,
    pub source: String,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub rates: Option<KaminoRates>,
    pub vaults: BTreeMap<String, FeedView>,
}

pub struct FeedUrls {
    pub phoenix: String,
    pub kamino: String,
    pub kamino_market: String,
    pub jupiter: String,
}

pub struct FeedVault {
    pub symbol: String,
    pub mint: String,
    pub phoenix_market: String,
}

pub struct LiveFeed {
    client: reqwest::Client,
    urls: FeedUrls,
    vaults: Vec<FeedVault>,
    calendars: BTreeMap<String, CalendarDoc>,
    pub snapshot: Snapshot,
}

impl LiveFeed {
    pub fn new(urls: FeedUrls, vaults: Vec<FeedVault>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .user_agent("carrera-keeper")
            .build()
            .expect("reqwest client");
        Self { client, urls, vaults, calendars: BTreeMap::new(), snapshot: Snapshot::default() }
    }

    async fn get(&self, url: &str) -> Result<String> {
        let r = self.client.get(url).send().await.with_context(|| format!("GET {url}"))?;
        let status = r.status();
        let body = r.text().await?;
        if !status.is_success() {
            return Err(anyhow!("GET {url} → {status}: {}", body.chars().take(200).collect::<String>()));
        }
        Ok(body)
    }

    /// Fetch everything once. Failures are logged per source; the previous value for
    /// that field is dropped (a stale number is worse than a skipped crank).
    pub async fn refresh(&mut self, now: DateTime<Utc>) {
        let fetched_ts = now.timestamp();
        let mut snap = Snapshot::default();
        let mut sources: Vec<&str> = Vec::new();

        // Kamino rates + implied prices.
        let kamino_url = format!("{}/kamino-market/{}/reserves/metrics", self.urls.kamino, self.urls.kamino_market);
        let mut kamino_prices = BTreeMap::new();
        match self.get(&kamino_url).await {
            Ok(body) => {
                match parse_kamino_rates(&body, "USDC") {
                    Ok(r) => {
                        snap.rates = Some(r);
                        sources.push("kamino");
                    }
                    Err(e) => tracing::warn!("feed: {e:#}"),
                }
                kamino_prices = parse_kamino_prices(&body).unwrap_or_default();
            }
            Err(e) => tracing::warn!("feed: kamino: {e:#}"),
        }

        // Jupiter prices for every mint in one call.
        let ids: Vec<&str> = self.vaults.iter().map(|v| v.mint.as_str()).collect();
        let jup_url = format!("{}?ids={}", self.urls.jupiter, ids.join(","));
        let jup_prices = match self.get(&jup_url).await {
            Ok(body) => match parse_jupiter_prices(&body) {
                Ok(p) => {
                    sources.push("jupiter");
                    p
                }
                Err(e) => {
                    tracing::warn!("feed: {e:#}");
                    BTreeMap::new()
                }
            },
            Err(e) => {
                tracing::warn!("feed: jupiter: {e:#}");
                BTreeMap::new()
            }
        };

        // Phoenix market status.
        let markets_url = format!("{}/v1/view/exchange/markets", self.urls.phoenix);
        let markets = match self.get(&markets_url).await {
            Ok(body) => match parse_markets(&body) {
                Ok(m) => {
                    sources.push("phoenix");
                    m
                }
                Err(e) => {
                    tracing::warn!("feed: {e:#}");
                    BTreeMap::new()
                }
            },
            Err(e) => {
                tracing::warn!("feed: phoenix markets: {e:#}");
                BTreeMap::new()
            }
        };

        let source = sources.join("+");
        for v in &self.vaults {
            // Funding: latest hourly point.
            let f_url = format!("{}/v1/funding/{}/rates?limit=3", self.urls.phoenix, v.phoenix_market);
            let funding = match self.get(&f_url).await {
                Ok(body) => match parse_funding_latest(&body) {
                    Ok(f) => f,
                    Err(e) => {
                        tracing::warn!("feed: {}: {e:#}", v.symbol);
                        None
                    }
                },
                Err(e) => {
                    tracing::warn!("feed: {} funding: {e:#}", v.symbol);
                    None
                }
            };
            if funding.is_none() {
                tracing::warn!("feed: {}: no funding sample this hour, record_funding will be skipped", v.symbol);
            }

            // Calendar: fetched once per market and kept (holiday overrides change rarely).
            if !self.calendars.contains_key(&v.phoenix_market) {
                let c_url = format!("{}/v1/market/{}/market-calendar", self.urls.phoenix, v.phoenix_market);
                match self.get(&c_url).await.and_then(|b| parse_calendar(&b)) {
                    Ok(c) => {
                        self.calendars.insert(v.phoenix_market.clone(), c);
                    }
                    Err(e) => tracing::warn!("feed: {} calendar: {e:#}", v.symbol),
                }
            }
            let cal_open = self.calendars.get(&v.phoenix_market).map(|c| calendar_cash_open(c, now));
            let mkt = markets.get(&v.phoenix_market);
            let phoenix_open = match (mkt, cal_open) {
                (Some(m), Some(c)) => Some(m.active && c),
                (Some(m), None) => Some(m.active),
                (None, Some(c)) => Some(c),
                (None, None) => None,
            };

            let price_e6 = jup_prices.get(&v.mint).copied().or_else(|| kamino_prices.get(&v.mint).copied());
            snap.vaults.insert(
                v.symbol.clone(),
                FeedView {
                    funding_hourly_scaled: funding.map(|f| f.1),
                    funding_ts: funding.map(|f| f.0),
                    borrow_bps: snap.rates.as_ref().map(|r| r.borrow_bps),
                    supply_bps: snap.rates.as_ref().map(|r| r.supply_bps),
                    price_e6,
                    phoenix_open,
                    fetched_ts,
                    source: source.clone(),
                },
            );
        }
        self.snapshot = snap;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const FUNDING: &str = include_str!("../tests/fixtures/phoenix_funding_tsla.json");
    const MARKETS: &str = include_str!("../tests/fixtures/phoenix_markets_slim.json");
    const CALENDAR: &str = include_str!("../tests/fixtures/phoenix_calendar_tsla.json");
    const KAMINO: &str = include_str!("../tests/fixtures/kamino_xstocks_metrics.json");
    const JUPITER: &str = include_str!("../tests/fixtures/jupiter_price_v3.json");

    #[test]
    fn funding_pct_conversion() {
        assert_eq!(funding_pct_to_scaled("0.004209"), Some(420_900));
        assert_eq!(funding_pct_to_scaled("-0.0010"), Some(-100_000));
        assert_eq!(funding_pct_to_scaled("0"), Some(0));
        assert_eq!(funding_pct_to_scaled("x"), None);
        // 0.004209 %/h × 8760 h ≈ 36.9 % a year; the program's f_avg is mean × 8760 / 1e6 in bps.
        assert_eq!(420_900_i64 * 8760 / 1_000_000, 3687);
    }

    #[test]
    fn funding_latest_is_max_timestamp() {
        let (ts, scaled) = parse_funding_latest(FUNDING).unwrap().unwrap();
        assert_eq!(ts, 1_790_290_801);
        assert!(scaled > 0 && scaled < 2_000_000, "{scaled}");
        assert_eq!(parse_funding_latest(r#"{"marketId":1,"symbol":"X","rates":[]}"#).unwrap(), None);
    }

    #[test]
    fn kamino_usdc_rates_and_prices() {
        let r = parse_kamino_rates(KAMINO, "USDC").unwrap();
        assert_eq!(r, KaminoRates { borrow_bps: 589, supply_bps: 478 });
        assert!(parse_kamino_rates(KAMINO, "NOPE").is_err());
        let p = parse_kamino_prices(KAMINO).unwrap();
        let tsla = p["XsDoVfqeBukxuZHWhdvWHBhgEHjGNst4MLodqsJHzoB"];
        assert!((370_000_000..390_000_000).contains(&tsla), "{tsla}");
        assert_eq!(apy_decimal_to_bps("0.0589"), Some(589));
        assert_eq!(apy_decimal_to_bps("-1"), None);
    }

    #[test]
    fn jupiter_prices() {
        let p = parse_jupiter_prices(JUPITER).unwrap();
        assert_eq!(p["EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"], 999_819);
        assert_eq!(p["XsDoVfqeBukxuZHWhdvWHBhgEHjGNst4MLodqsJHzoB"], 378_046_224);
        assert_eq!(usd_to_e6(0.0), None);
    }

    #[test]
    fn markets_status() {
        let m = parse_markets(MARKETS).unwrap();
        assert_eq!(m["TSLA"], MarketInfo { active: true, after_hours: false });
        assert!(m.contains_key("AAPL") && m.contains_key("SOL"));
    }

    #[test]
    fn calendar_cash_session_and_holiday() {
        let cal = parse_calendar(CALENDAR).unwrap();
        let ny = |y, m, d, h, mi| New_York.with_ymd_and_hms(y, m, d, h, mi, 0).unwrap().with_timezone(&Utc);
        // Thursday 24 Sep 2026: weekly schedule.
        assert!(!calendar_cash_open(&cal, ny(2026, 9, 24, 9, 29)));
        assert!(calendar_cash_open(&cal, ny(2026, 9, 24, 9, 30)));
        assert!(calendar_cash_open(&cal, ny(2026, 9, 24, 15, 59)));
        assert!(!calendar_cash_open(&cal, ny(2026, 9, 24, 16, 0)));
        assert!(!calendar_cash_open(&cal, ny(2026, 9, 26, 12, 0))); // Saturday
        // 19 Jun 2026 (Juneteenth) is an override with INTERNAL only.
        assert!(!calendar_cash_open(&cal, ny(2026, 6, 19, 12, 0)));
        // 2 Jul 2026 override keeps the cash session.
        assert!(calendar_cash_open(&cal, ny(2026, 7, 2, 12, 0)));
    }

    #[test]
    fn feed_view_serialises() {
        let v = FeedView { funding_hourly_scaled: Some(420_900), price_e6: Some(378_046_224), phoenix_open: Some(true), source: "kamino+jupiter+phoenix".into(), ..Default::default() };
        let j = serde_json::to_value(&v).unwrap();
        assert_eq!(j["funding_hourly_scaled"], 420_900);
        assert_eq!(j["borrow_bps"], serde_json::Value::Null);
    }
}
