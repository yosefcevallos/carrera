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
    accounts::{OverlayVault, VaultState},
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
use serde::Deserialize;
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeysJson {
    canonical_mint: String,
    global_vault: String,
    perp_asset_map: String,
    global_trader_index: Vec<String>,
    active_trader_buffer: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MarketJson {
    symbol: String,
    market_pubkey: String,
    spline_pubkey: String,
    base_lots_decimals: u8,
}

pub async fn fetch_phoenix_keys(client: &reqwest::Client, api: &str) -> Result<PhoenixKeys> {
    let keys: KeysJson = client.get(format!("{api}/v1/view/exchange/keys")).send().await?.error_for_status()?.json().await.context("exchange keys")?;
    let markets_raw: serde_json::Value = client.get(format!("{api}/v1/view/exchange/markets")).send().await?.error_for_status()?.json().await.context("markets")?;
    let list = match &markets_raw {
        serde_json::Value::Array(a) => a.clone(),
        serde_json::Value::Object(o) => o.get("markets").and_then(|m| m.as_array()).cloned().unwrap_or_default(),
        _ => Vec::new(),
    };
    let mut markets = Vec::new();
    for m in list {
        if let Ok(m) = serde_json::from_value::<MarketJson>(m) {
            markets.push(PhoenixMarket { symbol: m.symbol, orderbook: m.market_pubkey.parse()?, spline: m.spline_pubkey.parse()?, base_lots_decimals: m.base_lots_decimals });
        }
    }
    let parse_all = |v: &[String]| v.iter().map(|s| s.parse::<Pubkey>()).collect::<std::result::Result<Vec<_>, _>>();
    Ok(PhoenixKeys {
        canonical_mint: keys.canonical_mint.parse()?,
        global_vault: keys.global_vault.parse()?,
        perp_asset_map: keys.perp_asset_map.parse()?,
        global_trader_index: parse_all(&keys.global_trader_index)?,
        active_trader_buffer: parse_all(&keys.active_trader_buffer)?,
        markets,
        fetched: Instant::now(),
    })
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

/// The Phoenix block: 16 fixed accounts, then the index and buffer accounts.
pub fn phoenix_block(vault: &Pubkey, usdc_mint: &Pubkey, usdc_buffer: &Pubkey, keys: &PhoenixKeys, market: &PhoenixMarket) -> Vec<AccountMeta> {
    let w = |k: Pubkey| AccountMeta::new(k, false);
    let r = |k: Pubkey| AccountMeta::new_readonly(k, false);
    let mut v = vec![
        r(PHOENIX_PROGRAM_ID),
        r(PHOENIX_LOG_AUTHORITY),
        r(PHOENIX_GLOBAL_CONFIG),
        w(trader_account(vault)),
        w(keys.perp_asset_map),
        w(market.orderbook),
        w(market.spline),
        w(keys.global_vault),
        w(associated_token_address(vault, &keys.canonical_mint, &TOKEN_PROGRAM_ID)),
        r(keys.canonical_mint),
        r(EMBER_PROGRAM_ID),
        w(ember_state()),
        w(ember_vault()),
        r(*usdc_mint),
        w(*usdc_buffer),
        r(TOKEN_PROGRAM_ID),
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
    let fresh = fetch_phoenix_keys(&ctx.http, &ctx.cfg.phoenix_api_url).await?;
    *ctx.phoenix_keys.lock().await = Some(fresh.clone());
    Ok(fresh)
}

/// Build the venue args for one crank. Mock build: nothing. Real build: Kamino + Phoenix
/// blocks, a Jupiter route when `swap` says so, and the vault's lookup table.
pub async fn prepare(ctx: &Ctx, vc: &VaultCfg, v: &OverlayVault, swap: Swap) -> Result<Prepared> {
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
    remaining.extend(kamino.iter().cloned());
    remaining.extend(phoenix.iter().cloned());
    let mut blocks = BLOCK_KAMINO | BLOCK_PHOENIX;
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
    let data = VenueData {
        blocks,
        phoenix_gti: keys.global_trader_index.len() as u8,
        phoenix_atb: keys.active_trader_buffer.len() as u8,
        base_lot_size: base_lot_size(vc.stock_decimals, market.base_lots_decimals),
        price_in_ticks: 0,
        last_valid_slot: slot + ORDER_VALID_SLOTS,
        // D6: keeper-supplied. Until the trader account is decoded off-chain this is the
        // vault's own cached figure (kept current by the program on every Phoenix leg).
        phoenix_equity_usdc: v.phoenix_equity_usdc,
        client_order_id: chrono::Utc::now().timestamp() as u64,
        jupiter_data,
    };

    // Static addresses go in the vault's lookup table (created/extended on first use).
    let state_dir = alt::state_dir_from_lease(&ctx.cfg.lease_path);
    let mut static_keys: Vec<Pubkey> = kamino.iter().chain(phoenix.iter()).map(|m| m.pubkey).collect();
    static_keys.push(chain.pdas().registry());
    static_keys.push(vault);
    static_keys.push(ctx.cfg.program_id);
    let table = alt::ensure(chain, &state_dir, &vc.symbol, &static_keys).await?;
    tables.push(table);

    Ok(Prepared { args: VenueArgs::from_data(&data, remaining, Vec::new()), tables })
}

/// Prepare and send one crank; logs and returns false on any failure.
pub async fn send_crank<F>(ctx: &Ctx, vc: &VaultCfg, v: &OverlayVault, swap: Swap, label: &str, build: F) -> bool
where
    F: FnOnce(&VenueArgs) -> Instruction,
{
    let prepared = match prepare(ctx, vc, v, swap).await {
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
pub async fn send_crank_fresh<F>(ctx: &Ctx, vc: &VaultCfg, swap_for: impl FnOnce(&OverlayVault) -> Swap, label: &str, build: F) -> bool
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
    send_crank(ctx, vc, &v, swap, label, build).await
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

/// Stock `unwind_partial(fraction)` sells.
pub fn partial_sell_qty(v: &OverlayVault, fraction_bps: u32) -> u64 {
    (v.basis_spot_qty as u128 * fraction_bps as u128 / 10_000) as u64
}

/// USDC `size_up` deploys in Basis: the borrow increment to the tier LTV.
pub fn size_up_usdc(v: &OverlayVault, stock_decimals: u8) -> u64 {
    let target = v.collateral_value_usdc(stock_decimals) * v.params.ltv_bps as u128 / 10_000;
    target.saturating_sub(v.debt_usdc as u128 + v.debt_b_usdc as u128) as u64
}

/// Swap for a crank given the vault's current state.
pub fn swap_for_wind_step(v: &OverlayVault, n: u8) -> Swap {
    if n == 1 { Swap::UsdcToStock(wind_deploy_usdc(v)) } else { Swap::None }
}

pub fn swap_for_unwind_step(v: &OverlayVault, n: u8) -> Swap {
    if n == 3 { Swap::StockToUsdc(unwind_sell_qty(v)) } else { Swap::None }
}

pub fn swap_for_size_up(v: &OverlayVault, stock_decimals: u8) -> Swap {
    match v.state() {
        Ok(VaultState::Basis) => Swap::UsdcToStock(size_up_usdc(v, stock_decimals)),
        _ => Swap::None,
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
        let custody = chain.pdas().stock_custody(&vault);
        let buffer = chain.pdas().usdc_buffer(&vault);
        let mut metas = KaminoBlock::metas(&vault, &vc.mint, &custody, &buffer, &stock_reserve, &stock, &usdc_reserve, &usdc)?;
        metas.push(AccountMeta::new(user_metadata_address(&vault), false));
        let data = VenueData { blocks: BLOCK_KAMINO, ..Default::default() };
        let args = VenueArgs::from_data(&data, metas, Vec::new());
        let ix = chain.ix.init_kamino_obligation(&vault, &args);
        chain.send_ixs(&format!("{} init_kamino_obligation", vc.symbol), vec![ix], &[]).await?;
    }
    if !has_token {
        let ix = create_ata_idempotent(&chain.keeper(), &vault, &keys.canonical_mint, &TOKEN_PROGRAM_ID);
        chain.send_ixs(&format!("{} create phoenix token account", vc.symbol), vec![ix], &[]).await?;
    }
    if !has_trader {
        let ix = register_trader_ix(&chain.keeper(), &vault, &trader)?;
        chain.send_ixs(&format!("{} register_trader", vc.symbol), vec![ix], &[]).await?;
    }
    Ok(())
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
    fn phoenix_block_shape() {
        let keys = PhoenixKeys {
            canonical_mint: Pubkey::new_unique(),
            global_vault: Pubkey::new_unique(),
            perp_asset_map: Pubkey::new_unique(),
            global_trader_index: vec![Pubkey::new_unique()],
            active_trader_buffer: vec![Pubkey::new_unique(), Pubkey::new_unique()],
            markets: vec![],
            fetched: Instant::now(),
        };
        let m = PhoenixMarket { symbol: "TSLA".into(), orderbook: Pubkey::new_unique(), spline: Pubkey::new_unique(), base_lots_decimals: 3 };
        let vault = Pubkey::new_unique();
        let b = phoenix_block(&vault, &Pubkey::new_unique(), &Pubkey::new_unique(), &keys, &m);
        assert_eq!(b.len(), 16 + 1 + 2);
        assert_eq!(b[0].pubkey, PHOENIX_PROGRAM_ID);
        assert_eq!(b[3].pubkey, trader_account(&vault));
        assert_eq!(b[10].pubkey, EMBER_PROGRAM_ID);
        assert_eq!(b[15].pubkey, TOKEN_PROGRAM_ID);
        assert!(b[16].is_writable && b[17].is_writable);
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
