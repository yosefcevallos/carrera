//! Automatic venue blocks for the real program build.
//!
//! Every engine crank the program executes a venue leg for carries the account blocks it
//! needs in `remaining_accounts` plus a Borsh `VenueData` (docs/CONTRACT.md "Venue blocks").
//! Against the mock-venues build (`program_build = "mock"`) nothing is attached. Against the
//! real build this module assembles, per crank:
//!
//! - the 24-account **Kamino** block from the two reserves (always: every crank borrows,
//!   supplies, repays or withdraws through the obligation),
//! - the **Phoenix** block (fixed 16 + global-trader-index + active-trader-buffer accounts)
//!   from the exchange keys and the market list (always: cheap, and every step past
//!   `wind_step(1)` touches the trader account),
//! - a **Jupiter** route when the step swaps (`Swap`), quoted at send time with the vault as
//!   the authority and the vault's PDA token accounts substituted.
//!
//! The Kamino and Phoenix addresses are put in the vault's keeper-owned lookup table
//! (`alt.rs`), the Jupiter route brings its own tables, and `Chain::send_venue` compiles a
//! v0 message over all of them.
//!
//! One-time setup per vault, done lazily on first sight: the Kamino obligation
//! (`init_kamino_obligation`), the Phoenix trader account (`register_trader`, keeper pays;
//! the vault PDA is the trader authority and never needs to sign for registration) and the
//! vault's token account for the Phoenix collateral mint.

use crate::{
    accounts::OverlayVault,
    alt,
    chain::Chain,
    config::{Config, ProgramBuild, VaultCfg},
    venue_accounts::{
        associated_token_address, jupiter_route, obligation_address, user_metadata_address, KaminoBlock, ReserveInfo,
        VenueArgs, VenueData, BLOCK_JUPITER, BLOCK_KAMINO, BLOCK_PHOENIX, KLEND_PROGRAM_ID, TOKEN_PROGRAM_ID,
    },
    venue_setup::jupiter_base_url,
    Ctx,
};
use anyhow::{anyhow, Context as _, Result};
use serde::{Deserialize, Serialize};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
    signature::Signer,
};
use std::time::{Duration, Instant};

pub const PHOENIX_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("EtrnLzgbS7nMMy5fbD42kXiUzGg8XQzJ972Xtk1cjWih");
pub const PHOENIX_LOG_AUTHORITY: Pubkey = solana_sdk::pubkey!("GdxfTLSsdSY37G6fZoYtdGDSfgFnbT2EmRpuePZxWShS");
pub const PHOENIX_GLOBAL_CONFIG: Pubkey = solana_sdk::pubkey!("2zskx2iyCvb6Stg7RBZkt1f6MrF4dpYtMG3yMvKwqtUZ");
pub const EMBER_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("EMBERpYNE6ehWmXymZZS2skiFmCa9V5dp14e1iduM5qy");
pub const ASSOCIATED_TOKEN_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
pub const SYSTEM_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("11111111111111111111111111111111");

/// Cross-margin trader: pda index 0, subaccount 0.
pub const TRADER_PDA_INDEX: u8 = 0;
pub const TRADER_SUBACCOUNT: u8 = 0;
/// Phoenix's cross-margin position cap.
pub const TRADER_MAX_POSITIONS: u64 = 128;
/// Slots a market order stays valid.
pub const ORDER_VALID_SLOTS: u64 = 150;
const KEYS_MAX_AGE: Duration = Duration::from_secs(3600);

/// Which swap, if any, a crank performs; the keeper quotes a route for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Swap {
    None,
    /// USDC base units → stock.
    UsdcToStock(u64),
    /// Stock base units → USDC.
    StockToUsdc(u64),
}

/// Phoenix exchange keys plus per-market accounts, fetched from the public API.
#[derive(Clone, Debug)]
pub struct PhoenixKeys {
    pub canonical_mint: Pubkey,
    pub global_vault: Pubkey,
    pub perp_asset_map: Pubkey,
    pub global_trader_index: Vec<Pubkey>,
    pub active_trader_buffer: Vec<Pubkey>,
    pub withdraw_queue: Pubkey,
    pub markets: Vec<PhoenixMarket>,
    fetched: Instant,
}

#[derive(Clone, Debug)]
pub struct PhoenixMarket {
    pub symbol: String,
    pub orderbook: Pubkey,
    pub spline: Pubkey,
    pub base_lots_decimals: u8,
}

impl PhoenixKeys {
    pub fn market(&self, symbol: &str) -> Option<&PhoenixMarket> {
        self.markets.iter().find(|m| m.symbol.eq_ignore_ascii_case(symbol))
    }

    pub fn is_fresh(&self) -> bool {
        self.fetched.elapsed() < KEYS_MAX_AGE
    }
}

/// Phoenix's public API rate-limits bursts (HTTP 429); a migrate run spawns one keeper process per
/// vault, so the two exchange responses are also cached on disk for `KEYS_DISK_MAX_AGE` in the
/// state directory (`phoenix-keys.json`, next to the lease file).
const KEYS_DISK_MAX_AGE: Duration = Duration::from_secs(60);

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeysJson {
    canonical_mint: String,
    global_vault: String,
    perp_asset_map: String,
    global_trader_index: Vec<String>,
    active_trader_buffer: Vec<String>,
    withdraw_queue: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarketJson {
    symbol: String,
    market_pubkey: String,
    spline_pubkey: String,
    base_lots_decimals: u8,
}

#[derive(Serialize, Deserialize)]
struct KeysSnapshot {
    fetched_ts: i64,
    keys: KeysJson,
    markets: Vec<MarketJson>,
}

/// GET with exponential backoff on HTTP 429 (3 tries: 1 s, 2 s, 4 s).
async fn get_json_backoff<T: serde::de::DeserializeOwned>(client: &reqwest::Client, url: &str, what: &str) -> Result<T> {
    let mut delay = Duration::from_secs(1);
    for attempt in 1..=3 {
        let res = client.get(url).send().await.with_context(|| format!("{what}: request"))?;
        if res.status() == reqwest::StatusCode::TOO_MANY_REQUESTS && attempt < 3 {
            tracing::warn!("{what}: HTTP 429, retrying in {}s ({attempt}/3)", delay.as_secs());
            tokio::time::sleep(delay).await;
            delay *= 2;
            continue;
        }
        return res.error_for_status().with_context(|| format!("{what}: status"))?.json().await.with_context(|| format!("{what}: body"));
    }
    unreachable!()
}

fn build_keys(keys: KeysJson, markets_raw: Vec<MarketJson>) -> Result<PhoenixKeys> {
    let mut markets = Vec::new();
    for m in markets_raw {
        markets.push(PhoenixMarket { symbol: m.symbol, orderbook: m.market_pubkey.parse()?, spline: m.spline_pubkey.parse()?, base_lots_decimals: m.base_lots_decimals });
    }
    let parse_all = |v: &[String]| v.iter().map(|s| s.parse::<Pubkey>()).collect::<std::result::Result<Vec<_>, _>>();
    Ok(PhoenixKeys {
        canonical_mint: keys.canonical_mint.parse()?,
        global_vault: keys.global_vault.parse()?,
        perp_asset_map: keys.perp_asset_map.parse()?,
        global_trader_index: parse_all(&keys.global_trader_index)?,
        active_trader_buffer: parse_all(&keys.active_trader_buffer)?,
        withdraw_queue: keys.withdraw_queue.parse()?,
        markets,
        fetched: Instant::now(),
    })
}

fn markets_list(raw: serde_json::Value) -> Vec<MarketJson> {
    let list = match raw {
        serde_json::Value::Array(a) => a,
        serde_json::Value::Object(o) => o.get("markets").and_then(|m| m.as_array()).cloned().unwrap_or_default(),
        _ => Vec::new(),
    };
    list.into_iter().filter_map(|m| serde_json::from_value::<MarketJson>(m).ok()).collect()
}

async fn fetch_snapshot(client: &reqwest::Client, api: &str) -> Result<(KeysJson, Vec<MarketJson>)> {
    let keys: KeysJson = get_json_backoff(client, &format!("{api}/v1/view/exchange/keys"), "exchange keys").await?;
    let markets_raw: serde_json::Value = get_json_backoff(client, &format!("{api}/v1/view/exchange/markets"), "markets").await?;
    Ok((keys, markets_list(markets_raw)))
}

/// Exchange keys from disk when written within `KEYS_DISK_MAX_AGE`, else from the API (then
/// written to disk). `path` is `<state_dir>/phoenix-keys.json`.
pub async fn phoenix_keys_cached(client: &reqwest::Client, api: &str, path: &std::path::Path) -> Result<PhoenixKeys> {
    let now = chrono::Utc::now().timestamp();
    if let Ok(text) = std::fs::read_to_string(path) {
        if let Ok(snap) = serde_json::from_str::<KeysSnapshot>(&text) {
            if now - snap.fetched_ts < KEYS_DISK_MAX_AGE.as_secs() as i64 {
                if let Ok(k) = build_keys(snap.keys, snap.markets) {
                    return Ok(k);
                }
            }
        }
    }
    let (keys, markets) = fetch_snapshot(client, api).await?;
    let snap = KeysSnapshot { fetched_ts: now, keys, markets };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, serde_json::to_string(&snap)?).and_then(|_| std::fs::rename(&tmp, path)).is_err() {
        tracing::warn!("could not write {}", path.display());
    }
    build_keys(snap.keys, snap.markets)
}

/// The vault's Phoenix trader PDA: `["trader", vault, [pda_index, subaccount]]`.
pub fn trader_account(vault: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"trader", vault.as_ref(), &[TRADER_PDA_INDEX, TRADER_SUBACCOUNT]], &PHOENIX_PROGRAM_ID).0
}

pub fn ember_state() -> Pubkey {
    Pubkey::find_program_address(&[PHOENIX_PROGRAM_ID.as_ref(), b"state"], &EMBER_PROGRAM_ID).0
}

pub fn ember_vault() -> Pubkey {
    Pubkey::find_program_address(&[PHOENIX_PROGRAM_ID.as_ref(), b"vault"], &EMBER_PROGRAM_ID).0
}

/// Base units per Phoenix base lot for a stock with `stock_decimals` decimals.
pub fn base_lot_size(stock_decimals: u8, base_lots_decimals: u8) -> u64 {
    10u64.pow(stock_decimals.saturating_sub(base_lots_decimals) as u32)
}

/// The Phoenix block: 17 fixed accounts, then the index and buffer accounts.
pub fn phoenix_block(vault: &Pubkey, usdc_mint: &Pubkey, usdc_buffer: &Pubkey, keys: &PhoenixKeys, market: &PhoenixMarket) -> Vec<AccountMeta> {
    let w = |k: Pubkey| AccountMeta::new(k, false);
    let r = |k: Pubkey| AccountMeta::new_readonly(k, false);
    // Phoenix's deposit / withdraw / order instructions take the global configuration writable,
    // and Ember mints / burns the canonical token, so both are writable in the outer transaction
    // (a CPI cannot escalate a readonly outer account).
    let mut v = vec![
        r(PHOENIX_PROGRAM_ID),
        r(PHOENIX_LOG_AUTHORITY),
        w(PHOENIX_GLOBAL_CONFIG),
        w(trader_account(vault)),
        w(keys.perp_asset_map),
        w(market.orderbook),
        w(market.spline),
        w(keys.global_vault),
        w(associated_token_address(vault, &keys.canonical_mint, &TOKEN_PROGRAM_ID)),
        w(keys.canonical_mint),
        r(EMBER_PROGRAM_ID),
        w(ember_state()),
        w(ember_vault()),
        r(*usdc_mint),
        w(*usdc_buffer),
        r(TOKEN_PROGRAM_ID),
        w(keys.withdraw_queue),
    ];
    v.extend(keys.global_trader_index.iter().map(|k| w(*k)));
    v.extend(keys.active_trader_buffer.iter().map(|k| w(*k)));
    v
}

/// Everything a crank needs: the venue args for the instruction and the lookup tables.
pub struct Prepared {
    pub args: VenueArgs,
    pub tables: Vec<Pubkey>,
}

impl Prepared {
    pub fn none() -> Self {
        Self { args: VenueArgs::none(), tables: Vec::new() }
    }
}

/// Fetch both reserves (cached per pass would be nicer; two account reads is fine hourly).
async fn reserves(chain: &Chain, cfg: &Config, vc: &VaultCfg) -> Result<(Pubkey, ReserveInfo, Pubkey, ReserveInfo)> {
    let stock_reserve = vc.kamino_reserve.ok_or_else(|| anyhow!("{}: kamino_reserve not configured", vc.symbol))?;
    let usdc_reserve = cfg.kamino_reserve;
    if usdc_reserve == Pubkey::default() {
        return Err(anyhow!("kamino_reserve (USDC) not configured"));
    }
    let accs = chain.rpc.get_multiple_accounts(&[stock_reserve, usdc_reserve]).await.context("reserves")?;
    let stock = ReserveInfo::parse(&accs[0].as_ref().ok_or_else(|| anyhow!("stock reserve missing"))?.data)?;
    let usdc = ReserveInfo::parse(&accs[1].as_ref().ok_or_else(|| anyhow!("usdc reserve missing"))?.data)?;
    Ok((stock_reserve, stock, usdc_reserve, usdc))
}

/// Phoenix keys, refreshed hourly and shared through the context.
pub async fn phoenix_keys(ctx: &Ctx) -> Result<PhoenixKeys> {
    {
        let cached = ctx.phoenix_keys.lock().await;
        if let Some(k) = cached.as_ref() {
            if k.is_fresh() {
                return Ok(k.clone());
            }
        }
    }
    let path = alt::state_dir_from_lease(&ctx.cfg.lease_path).join("phoenix-keys.json");
    let fresh = phoenix_keys_cached(&ctx.http, &ctx.cfg.phoenix_api_url, &path).await?;
    *ctx.phoenix_keys.lock().await = Some(fresh.clone());
    Ok(fresh)
}

/// The vault's keeper-owned lookup table holding the Kamino and Phoenix blocks, the registry,
/// the vault, the vault's Kamino user metadata and the program id; created / extended on first
/// use and persisted next to the lease file. Waits until new entries are usable.
pub async fn ensure_vault_table(ctx: &Ctx, vc: &VaultCfg, vault: &Pubkey, kamino: &[AccountMeta], phoenix: &[AccountMeta]) -> Result<Pubkey> {
    let chain = &ctx.chain;
    let state_dir = alt::state_dir_from_lease(&ctx.cfg.lease_path);
    let mut static_keys: Vec<Pubkey> = kamino.iter().chain(phoenix.iter()).map(|m| m.pubkey).collect();
    static_keys.push(chain.pdas().registry());
    static_keys.push(*vault);
    static_keys.push(user_metadata_address(vault));
    static_keys.push(ctx.cfg.program_id);
    alt::ensure(chain, &state_dir, &vc.symbol, &static_keys).await
}

/// Which blocks a crank needs. Mainnet allows 64 account locks per transaction (the
/// `increase_tx_account_lock_limit` feature is not active), and Kamino + Phoenix + a Jupiter
/// route is ~70–85 unique accounts, so every crank gets only the blocks its instruction
/// consumes (see `docs/CONTRACT.md` "Venue blocks" for the per-instruction table).
pub const NEED_NONE: u8 = 0;
pub const NEED_K: u8 = BLOCK_KAMINO;
pub const NEED_P: u8 = BLOCK_PHOENIX;
pub const NEED_KP: u8 = BLOCK_KAMINO | BLOCK_PHOENIX;

/// wind_step 1: Kamino withdraw/deposit + Jupiter; 2: Kamino borrow + Phoenix deposit; 3: Phoenix order.
pub fn blocks_for_wind_step(n: u8) -> u8 {
    match n {
        1 => NEED_K,
        2 => NEED_KP,
        _ => NEED_P,
    }
}

/// unwind_step 1: Phoenix close; 2: Phoenix withdraw + Kamino repay; 3: Kamino withdraw + Jupiter.
pub fn blocks_for_unwind_step(n: u8) -> u8 {
    match n {
        1 => NEED_P,
        2 => NEED_KP,
        _ => NEED_K,
    }
}

/// Build the venue args for one crank. Mock build: nothing. Real build: the requested Kamino /
/// Phoenix blocks, a Jupiter route when `swap` says so, and the vault's lookup table.
pub async fn prepare(ctx: &Ctx, vc: &VaultCfg, v: &OverlayVault, swap: Swap, need: u8) -> Result<Prepared> {
    if ctx.cfg.program_build == ProgramBuild::Mock {
        return Ok(Prepared::none());
    }
    let chain = &ctx.chain;
    let vault = chain.pdas().vault(&vc.mint);
    let custody = chain.pdas().stock_custody(&vault);
    let buffer = chain.pdas().usdc_buffer(&vault);

    let (stock_reserve, stock, usdc_reserve, usdc) = reserves(chain, &ctx.cfg, vc).await?;
    let kamino = KaminoBlock::metas(&vault, &vc.mint, &custody, &buffer, &stock_reserve, &stock, &usdc_reserve, &usdc)?;

    let keys = phoenix_keys(ctx).await?;
    let market = keys.market(&vc.phoenix_market).ok_or_else(|| anyhow!("{}: Phoenix market {} not listed", vc.symbol, vc.phoenix_market))?.clone();
    let phoenix = phoenix_block(&vault, &ctx.cfg.usdc_mint, &buffer, &keys, &market);

    let mut remaining = Vec::with_capacity(kamino.len() + phoenix.len() + 40);
    let mut blocks = 0u8;
    if need & BLOCK_KAMINO != 0 {
        remaining.extend(kamino.iter().cloned());
        blocks |= BLOCK_KAMINO;
    }
    if need & BLOCK_PHOENIX != 0 {
        remaining.extend(phoenix.iter().cloned());
        blocks |= BLOCK_PHOENIX;
    }
    let mut jupiter_data = Vec::new();
    let mut tables = Vec::new();

    match swap {
        Swap::None => {}
        Swap::UsdcToStock(amount) | Swap::StockToUsdc(amount) => {
            let (input, output, src, dst) = match swap {
                Swap::UsdcToStock(_) => (ctx.cfg.usdc_mint, vc.mint, buffer, custody),
                _ => (vc.mint, ctx.cfg.usdc_mint, custody, buffer),
            };
            let amount = amount.max(1);
            let base = jupiter_base_url(&ctx.cfg.jupiter_price_url);
            let route = jupiter_route(&ctx.http, &base, &input, &output, amount, v.params.max_swap_slippage_bps as u16, &vault, &src, &dst).await?;
            remaining.extend(route.block.iter().cloned());
            jupiter_data = route.data;
            tables.extend(route.lookup_tables);
            blocks |= BLOCK_JUPITER;
        }
    }

    let slot = chain.slot().await?;
    // D6: keeper-supplied equity, read from the trader account now (settled collateral less any
    // unsettled funding the vault owes); zero before the trader is registered.
    let equity = live_equity(ctx, &vault).await?.map(|e| e.withdrawable_usdc()).unwrap_or(0);
    if equity != v.phoenix_equity_usdc {
        tracing::info!("{}: live Phoenix equity {equity} vs cached {}", vc.symbol, v.phoenix_equity_usdc);
    }
    let data = VenueData {
        blocks,
        phoenix_gti: keys.global_trader_index.len() as u8,
        phoenix_atb: keys.active_trader_buffer.len() as u8,
        base_lot_size: base_lot_size(vc.stock_decimals, market.base_lots_decimals),
        price_in_ticks: 0,
        last_valid_slot: slot + ORDER_VALID_SLOTS,
        phoenix_equity_usdc: equity,
        client_order_id: chrono::Utc::now().timestamp() as u64,
        jupiter_data,
    };

    // Static addresses go in the vault's lookup table (created/extended on first use).
    let table = ensure_vault_table(ctx, vc, &vault, &kamino, &phoenix).await?;
    tables.push(table);

    Ok(Prepared { args: VenueArgs::from_data(&data, remaining, Vec::new()), tables })
}

/// Prepare and send one crank; logs and returns false on any failure.
pub async fn send_crank<F>(ctx: &Ctx, vc: &VaultCfg, v: &OverlayVault, swap: Swap, need: u8, label: &str, build: F) -> bool
where
    F: FnOnce(&VenueArgs) -> Instruction,
{
    let prepared = match prepare(ctx, vc, v, swap, need).await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("{label}: venue setup failed: {e:#}");
            return false;
        }
    };
    let ix = build(&prepared.args);
    ctx.chain.try_send_venue(label, ix, &prepared.args, &prepared.tables).await
}

/// Read the vault fresh, then prepare and send. For steps whose amounts depend on the
/// previous step's outcome.
pub async fn send_crank_fresh<F>(ctx: &Ctx, vc: &VaultCfg, swap_for: impl FnOnce(&OverlayVault) -> Swap, need: u8, label: &str, build: F) -> bool
where
    F: FnOnce(&VenueArgs) -> Instruction,
{
    let v = match ctx.chain.vault(&vc.mint).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("{label}: vault read failed: {e:#}");
            return false;
        }
    };
    let swap = swap_for(&v);
    send_crank(ctx, vc, &v, swap, need, label, build).await
}

// ------------------------------------------------------------ swap sizing

/// USDC the next `wind_step(1)` will deploy: the parked balance capped by the basis cap.
pub fn wind_deploy_usdc(v: &OverlayVault) -> u64 {
    let cap = v.params.basis_cap_usdc;
    if cap == 0 {
        v.parked_usdc
    } else {
        v.parked_usdc.min(cap.saturating_sub(v.debt_b_usdc))
    }
}

/// Stock `unwind_step(3)` sells: the whole basis spot leg.
pub fn unwind_sell_qty(v: &OverlayVault) -> u64 {
    v.basis_spot_qty
}

/// Stock `unwind_partial_step(3, fraction)` sells.
pub fn partial_sell_qty(v: &OverlayVault, fraction_bps: u32) -> u64 {
    (v.basis_spot_qty as u128 * fraction_bps as u128 / 10_000) as u64
}

pub fn swap_for_wind_step(v: &OverlayVault, n: u8) -> Swap {
    if n == 1 { Swap::UsdcToStock(wind_deploy_usdc(v)) } else { Swap::None }
}

pub fn swap_for_unwind_step(v: &OverlayVault, n: u8) -> Swap {
    if n == 3 { Swap::StockToUsdc(unwind_sell_qty(v)) } else { Swap::None }
}

/// Swap for `unwind_partial_step(n, fraction)`: only step 3 sells.
pub fn swap_for_partial_step(v: &OverlayVault, n: u8, fraction_bps: u32) -> Swap {
    if n == 3 {
        Swap::StockToUsdc(partial_sell_qty(v, fraction_bps))
    } else {
        Swap::None
    }
}

/// The vault's trader account decoded from chain; `None` before registration.
pub async fn live_equity(ctx: &Ctx, vault: &Pubkey) -> Result<Option<crate::phoenix_equity::TraderEquity>> {
    let trader = trader_account(vault);
    let accs = ctx.chain.rpc.get_multiple_accounts(&[trader]).await.context("trader account")?;
    match accs.into_iter().next().flatten() {
        Some(a) if a.data.len() > 8 => Ok(Some(crate::phoenix_equity::decode(&a.data)?)),
        _ => Ok(None),
    }
}


// ------------------------------------------------------------ one-time setup

/// Make sure the vault has its Kamino obligation, its Phoenix trader account and its
/// Phoenix collateral token account. Idempotent; a no-op on the mock build.
pub async fn ensure_setup(ctx: &Ctx, vc: &VaultCfg) -> Result<()> {
    if ctx.cfg.program_build == ProgramBuild::Mock {
        return Ok(());
    }
    let chain = &ctx.chain;
    let vault = chain.pdas().vault(&vc.mint);
    let (stock_reserve, stock, usdc_reserve, usdc) = reserves(chain, &ctx.cfg, vc).await?;
    let obligation = obligation_address(&vault, &stock.lending_market);
    let keys = phoenix_keys(ctx).await?;
    let trader = trader_account(&vault);
    let trader_token = associated_token_address(&vault, &keys.canonical_mint, &TOKEN_PROGRAM_ID);

    let accs = chain.rpc.get_multiple_accounts(&[obligation, trader, trader_token]).await.context("setup accounts")?;
    let (has_obligation, has_trader, has_token) = (accs[0].is_some(), accs[1].is_some(), accs[2].is_some());

    if !has_obligation {
        // The 24-account Kamino block plus the user-metadata PDA does not fit a packet with raw
        // keys (1656 bytes on mainnet), so the vault's lookup table is created and filled first
        // and the instruction is sent through it.
        let custody = chain.pdas().stock_custody(&vault);
        let buffer = chain.pdas().usdc_buffer(&vault);
        let kamino = KaminoBlock::metas(&vault, &vc.mint, &custody, &buffer, &stock_reserve, &stock, &usdc_reserve, &usdc)?;
        let market = keys.market(&vc.phoenix_market).ok_or_else(|| anyhow!("{}: Phoenix market {} not listed", vc.symbol, vc.phoenix_market))?.clone();
        let phoenix = phoenix_block(&vault, &ctx.cfg.usdc_mint, &buffer, &keys, &market);
        let table = ensure_vault_table(ctx, vc, &vault, &kamino, &phoenix).await?;
        let mut metas = kamino;
        metas.push(AccountMeta::new(user_metadata_address(&vault), false));
        let data = VenueData { blocks: BLOCK_KAMINO, ..Default::default() };
        let args = VenueArgs::from_data(&data, metas, Vec::new());
        let ix = chain.ix.init_kamino_obligation(&vault, &args);
        chain.send_ixs(&format!("{} init_kamino_obligation", vc.symbol), vec![ix], &[table]).await?;
    }
    if !has_token {
        let ix = create_ata_idempotent(&chain.keeper(), &vault, &keys.canonical_mint, &TOKEN_PROGRAM_ID);
        chain.send_ixs(&format!("{} create phoenix token account", vc.symbol), vec![ix], &[]).await?;
    }
    if !has_trader {
        // Registration alone leaves the trader without deposit / withdraw / risk-increase
        // capabilities; Phoenix's onboarder grants them in the same transaction through the
        // no-referral flow, which accepts PDA authorities (docs.phoenix.trade/sdk/register).
        let sig = onboard_trader(ctx, &vault).await?;
        tracing::info!(%sig, "{} register + onboard trader {trader}", vc.symbol);
    }
    Ok(())
}

/// Register + onboarding instructions for `vault`'s default trader account, built by Phoenix
/// (`POST /v1/exchange/build-register-ixs`). The onboarder is a signer of the second one.
pub struct RegisterIxs {
    pub instructions: Vec<Instruction>,
    pub trader_onboarder: Pubkey,
    pub trader_pda: Pubkey,
    pub include_register_trader: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiAccountMeta {
    pubkey: String,
    is_signer: bool,
    is_writable: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiInstruction {
    program_id: String,
    keys: Vec<ApiAccountMeta>,
    data: Vec<u8>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuildRegisterIxsResponse {
    instructions: Vec<ApiInstruction>,
    trader_pda: String,
    trader_onboarder: String,
    include_register_trader: bool,
}

pub async fn phoenix_register_ixs(ctx: &Ctx, vault: &Pubkey, payer: &Pubkey) -> Result<RegisterIxs> {
    let body = serde_json::json!({ "traderAuthority": vault.to_string(), "txFeePayer": payer.to_string() });
    let res: BuildRegisterIxsResponse = ctx
        .http
        .post(format!("{}/v1/exchange/build-register-ixs", ctx.cfg.phoenix_api_url))
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .context("build-register-ixs")?;
    let mut instructions = Vec::new();
    for ix in res.instructions {
        let accounts = ix
            .keys
            .iter()
            .map(|k| Ok(AccountMeta { pubkey: k.pubkey.parse()?, is_signer: k.is_signer, is_writable: k.is_writable }))
            .collect::<Result<Vec<_>>>()?;
        instructions.push(Instruction { program_id: ix.program_id.parse()?, accounts, data: ix.data });
    }
    Ok(RegisterIxs {
        instructions,
        trader_onboarder: res.trader_onboarder.parse()?,
        trader_pda: res.trader_pda.parse()?,
        include_register_trader: res.include_register_trader,
    })
}

/// Register and onboard the vault's trader: the keeper signs as fee payer, Phoenix adds the
/// onboarder signature and submits (`POST /v1/exchange/send-register-ixs`). Pays the trader
/// account's rent from the keeper. Returns the transaction signature.
pub async fn onboard_trader(ctx: &Ctx, vault: &Pubkey) -> Result<String> {
    use base64::Engine as _;
    use solana_sdk::{message::Message, signature::Signer, transaction::Transaction};
    let chain = &ctx.chain;
    let payer = chain.keeper();
    let built = phoenix_register_ixs(ctx, vault, &payer).await?;
    let expected = trader_account(vault);
    if built.trader_pda != expected {
        return Err(anyhow!("Phoenix derived trader {} but the keeper expects {expected}", built.trader_pda));
    }
    let bh = chain.rpc.get_latest_blockhash().await.context("blockhash")?;
    let mut tx = Transaction::new_unsigned(Message::new(&built.instructions, Some(&payer)));
    tx.partial_sign(&[&chain.payer], bh);
    let wire = base64::engine::general_purpose::STANDARD.encode(bincode::serialize(&tx).context("serialize")?);
    let body = serde_json::json!({ "transaction": wire, "traderAuthority": vault.to_string(), "txFeePayer": payer.to_string() });
    let res: serde_json::Value = ctx
        .http
        .post(format!("{}/v1/exchange/send-register-ixs", ctx.cfg.phoenix_api_url))
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .context("send-register-ixs")?;
    let sig = res["signature"].as_str().ok_or_else(|| anyhow!("send-register-ixs: no signature in {res}"))?.to_string();
    let _ = chain.payer.pubkey();
    Ok(sig)
}

/// Whether `sync_collateral` is due: stock sits in custody that the obligation does not hold.
pub async fn custody_balance(chain: &Chain, vc: &VaultCfg) -> Result<u64> {
    let vault = chain.pdas().vault(&vc.mint);
    let custody = chain.pdas().stock_custody(&vault);
    let bal = chain.rpc.get_token_account_balance(&custody).await.context("custody balance")?;
    Ok(bal.amount.parse().unwrap_or(0))
}

/// Phoenix `register_trader`: `[phoenix_program, log_authority, global_config, payer(ws), trader(r), trader_account(w), system]`.
pub fn register_trader_ix(payer: &Pubkey, trader: &Pubkey, trader_account: &Pubkey) -> Result<Instruction> {
    use phoenix_rise_ix::register_trader::{create_register_trader_ix, RegisterTraderParams};
    let to = |p: &Pubkey| solana_pubkey::Pubkey::new_from_array(p.to_bytes());
    let params = RegisterTraderParams::builder()
        .payer(to(payer))
        .trader(to(trader))
        .trader_account(to(trader_account))
        .max_positions(TRADER_MAX_POSITIONS)
        .trader_pda_index(TRADER_PDA_INDEX)
        .subaccount_index(TRADER_SUBACCOUNT)
        .build()
        .map_err(|e| anyhow!("register_trader params: {e:?}"))?;
    let ix = create_register_trader_ix(params).map_err(|e| anyhow!("register_trader: {e:?}"))?;
    Ok(Instruction {
        program_id: Pubkey::new_from_array(ix.program_id.to_bytes()),
        accounts: ix.accounts.into_iter().map(|m| AccountMeta { pubkey: Pubkey::new_from_array(m.pubkey.to_bytes()), is_signer: m.is_signer, is_writable: m.is_writable }).collect(),
        data: ix.data,
    })
}

/// Associated-token-account create (idempotent) for any owner, classic or Token-2022 mint.
pub fn create_ata_idempotent(payer: &Pubkey, owner: &Pubkey, mint: &Pubkey, token_program: &Pubkey) -> Instruction {
    let ata = associated_token_address(owner, mint, token_program);
    Instruction {
        program_id: ASSOCIATED_TOKEN_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(ata, false),
            AccountMeta::new_readonly(*owner, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(SYSTEM_PROGRAM_ID, false),
            AccountMeta::new_readonly(*token_program, false),
        ],
        data: vec![1],
    }
}

/// Remaining accounts for `refresh_nav` on the real build: `[klend, lending_market, scope_prices]`
/// so the program refreshes the reserve before reading its price; the oracle account is the
/// xStock reserve itself.
pub async fn refresh_nav_accounts(chain: &Chain, cfg: &Config, vc: &VaultCfg) -> Result<(Pubkey, Vec<AccountMeta>)> {
    let (stock_reserve, stock, _, _) = reserves(chain, cfg, vc).await?;
    Ok((
        stock_reserve,
        vec![
            AccountMeta::new_readonly(KLEND_PROGRAM_ID, false),
            AccountMeta::new_readonly(stock.lending_market, false),
            AccountMeta::new_readonly(stock.scope_price_feed, false),
        ],
    ))
}

#[allow(dead_code)]
fn _keeper_signer_is_pubkey(chain: &Chain) -> Pubkey {
    chain.payer.pubkey()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trader_pda_uses_index_and_subaccount() {
        let vault = Pubkey::new_unique();
        let expected = Pubkey::find_program_address(&[b"trader", vault.as_ref(), &[0u8, 0u8]], &PHOENIX_PROGRAM_ID).0;
        assert_eq!(trader_account(&vault), expected);
    }

    #[test]
    fn lot_size_is_decimal_difference() {
        assert_eq!(base_lot_size(8, 3), 100_000);
        assert_eq!(base_lot_size(8, 2), 1_000_000);
        assert_eq!(base_lot_size(6, 6), 1);
    }

    #[test]
    fn keys_snapshot_round_trips_and_expires() {
        let dir = std::env::temp_dir().join(format!("carrera-keys-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("phoenix-keys.json");
        let keys = KeysJson {
            canonical_mint: Pubkey::new_unique().to_string(),
            global_vault: Pubkey::new_unique().to_string(),
            perp_asset_map: Pubkey::new_unique().to_string(),
            global_trader_index: vec![Pubkey::new_unique().to_string()],
            active_trader_buffer: vec![Pubkey::new_unique().to_string()],
            withdraw_queue: Pubkey::new_unique().to_string(),
        };
        let markets = vec![MarketJson { symbol: "TSLA".into(), market_pubkey: Pubkey::new_unique().to_string(), spline_pubkey: Pubkey::new_unique().to_string(), base_lots_decimals: 3 }];
        let now = chrono::Utc::now().timestamp();
        let snap = KeysSnapshot { fetched_ts: now - 30, keys, markets };
        std::fs::write(&path, serde_json::to_string(&snap).unwrap()).unwrap();
        // Fresh: served from disk without any network (a URL that cannot resolve proves it).
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = reqwest::Client::new();
        let k = rt.block_on(phoenix_keys_cached(&client, "http://phoenix.invalid", &path)).unwrap();
        assert_eq!(k.market("tsla").unwrap().base_lots_decimals, 3);
        assert_eq!(k.withdraw_queue.to_string(), snap.keys.withdraw_queue);
        // Stale: the API is consulted (and fails here), the stale file is not used.
        let stale = KeysSnapshot { fetched_ts: now - 61, ..snap };
        std::fs::write(&path, serde_json::to_string(&stale).unwrap()).unwrap();
        assert!(rt.block_on(phoenix_keys_cached(&client, "http://phoenix.invalid", &path)).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn phoenix_block_shape() {
        let keys = PhoenixKeys {
            canonical_mint: Pubkey::new_unique(),
            global_vault: Pubkey::new_unique(),
            perp_asset_map: Pubkey::new_unique(),
            global_trader_index: vec![Pubkey::new_unique()],
            active_trader_buffer: vec![Pubkey::new_unique(), Pubkey::new_unique()],
            withdraw_queue: Pubkey::new_unique(),
            markets: vec![],
            fetched: Instant::now(),
        };
        let m = PhoenixMarket { symbol: "TSLA".into(), orderbook: Pubkey::new_unique(), spline: Pubkey::new_unique(), base_lots_decimals: 3 };
        let vault = Pubkey::new_unique();
        let b = phoenix_block(&vault, &Pubkey::new_unique(), &Pubkey::new_unique(), &keys, &m);
        assert_eq!(b.len(), 17 + 1 + 2);
        assert_eq!(b[0].pubkey, PHOENIX_PROGRAM_ID);
        assert_eq!(b[3].pubkey, trader_account(&vault));
        assert_eq!(b[10].pubkey, EMBER_PROGRAM_ID);
        assert_eq!(b[15].pubkey, TOKEN_PROGRAM_ID);
        assert!(b[16].is_writable && b[17].is_writable && b[18].is_writable);
    }

    #[test]
    fn swap_sizing() {
        let mut v = OverlayVault { parked_usdc: 5_000_000, ..Default::default() };
        assert_eq!(wind_deploy_usdc(&v), 5_000_000);
        v.params.basis_cap_usdc = 3_000_000;
        assert_eq!(wind_deploy_usdc(&v), 3_000_000);
        v.basis_spot_qty = 1_000_000;
        assert_eq!(partial_sell_qty(&v, 2_500), 250_000);
        assert_eq!(swap_for_wind_step(&v, 1), Swap::UsdcToStock(3_000_000));
        assert_eq!(swap_for_wind_step(&v, 2), Swap::None);
        assert_eq!(swap_for_unwind_step(&v, 3), Swap::StockToUsdc(1_000_000));
    }

    #[test]
    fn register_trader_accounts() {
        let (payer, vault) = (Pubkey::new_unique(), Pubkey::new_unique());
        let ix = register_trader_ix(&payer, &vault, &trader_account(&vault)).unwrap();
        assert_eq!(ix.program_id, PHOENIX_PROGRAM_ID);
        assert_eq!(ix.accounts.len(), 7);
        assert_eq!(ix.accounts[1].pubkey, PHOENIX_LOG_AUTHORITY);
        assert_eq!(ix.accounts[2].pubkey, PHOENIX_GLOBAL_CONFIG);
        assert!(ix.accounts[3].is_signer && ix.accounts[3].is_writable);
        assert_eq!(ix.accounts[4].pubkey, vault);
        assert!(!ix.accounts[4].is_signer);
        assert_eq!(ix.accounts[5].pubkey, trader_account(&vault));
    }
}
