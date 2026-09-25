//! Phoenix perps execution legs via `phoenix-rise-ix` builders (spec §7.4):
//! Ember wrap/unwrap between USDC and the Phoenix collateral token, deposit and
//! withdraw of that collateral into the vault's trader account, and bounded
//! immediate-or-cancel orders for the short leg.
//!
//! Per DECISIONS D6 the program does not deserialise Phoenix accounts: funding
//! and subaccount equity are keeper-supplied (`VenueData::phoenix_equity_usdc`)
//! and the base-lot size comes from `VenueData::base_lot_size`.
//!
//! # Phoenix block (`VenueData::blocks & BLOCK_PHOENIX`)
//! ```text
//!  0 phoenix_program              EtrnLzgbS7nMMy5fbD42kXiUzGg8XQzJ972Xtk1cjWih
//!  1 log_authority                GdxfTLSsdSY37G6fZoYtdGDSfgFnbT2EmRpuePZxWShS
//!  2 global_configuration         2zskx2iyCvb6Stg7RBZkt1f6MrF4dpYtMG3yMvKwqtUZ
//!  3 trader_account               the vault's registered Phoenix trader PDA
//!  4 perp_asset_map               the market
//!  5 orderbook
//!  6 spline_collection            PDA ["spline", market] of Phoenix
//!  7 global_vault                 PDA ["vault", canonical_mint] of Phoenix
//!  8 trader_phoenix_token_account vault-owned token account of the canonical mint
//!  9 canonical_mint               the Phoenix collateral token mint
//! 10 ember_program                EMBERpYNE6ehWmXymZZS2skiFmCa9V5dp14e1iduM5qy
//! 11 ember_state                  PDA [phoenix_program, "state"] of Ember
//! 12 ember_vault                  PDA [phoenix_program, "vault"] of Ember
//! 13 usdc_mint
//! 14 usdc_buffer                  vault PDA ["usdc", vault]
//! 15 token_program                classic SPL Token
//! 16.. global_trader_index accounts (`phoenix_gti` of them), then active_trader_buffer accounts (`phoenix_atb`)
//! ```
//! Orders are all-or-nothing: `min_base_lots_to_fill = num_base_lots`, so a fill
//! below the requested size fails inside Phoenix rather than leaving the hedge short.

use super::{token_amount, VenueCtx};
use crate::errors::CarreraError;
use crate::state::OverlayVault;
use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::{AccountMeta, Instruction};
use phoenix_rise_ix::deposit_funds::DepositFundsParams;
use phoenix_rise_ix::ember_deposit::EmberDepositParams;
use phoenix_rise_ix::ember_withdraw::EmberWithdrawParams;
use phoenix_rise_ix::market_order::MarketOrderParams;
use phoenix_rise_ix::types::{OrderFlags, SelfTradeBehavior, Side};
use phoenix_rise_ix::withdraw_funds::WithdrawFundsParams;
use solana_pubkey::Pubkey as RisePubkey;

pub const PHOENIX_PROGRAM_ID: Pubkey = pubkey!("EtrnLzgbS7nMMy5fbD42kXiUzGg8XQzJ972Xtk1cjWih");
pub const EMBER_PROGRAM_ID: Pubkey = pubkey!("EMBERpYNE6ehWmXymZZS2skiFmCa9V5dp14e1iduM5qy");

struct P<'a, 'info> {
    b: &'a [AccountInfo<'info>],
    gti: usize,
    atb: usize,
}

impl<'a, 'info> P<'a, 'info> {
    fn load(ctx: &VenueCtx<'a, 'info>) -> Result<Self> {
        let b = ctx.phoenix()?;
        let p = Self { b, gti: ctx.data.phoenix_gti as usize, atb: ctx.data.phoenix_atb as usize };
        require_keys_eq!(*p.b[0].key, PHOENIX_PROGRAM_ID, CarreraError::VenueAccountsMismatch);
        require_keys_eq!(*p.b[10].key, EMBER_PROGRAM_ID, CarreraError::VenueAccountsMismatch);
        let (buffer, _) = Pubkey::find_program_address(&[b"usdc", ctx.vault.key.as_ref()], &crate::ID);
        require_keys_eq!(*p.b[14].key, buffer, CarreraError::VenueAccountsMismatch);
        require!(p.gti > 0 && p.atb > 0, CarreraError::VenueAccountsMissing);
        Ok(p)
    }
    fn trader_account(&self) -> RisePubkey { r(self.b[3].key) }
    fn perp_asset_map(&self) -> RisePubkey { r(self.b[4].key) }
    fn orderbook(&self) -> RisePubkey { r(self.b[5].key) }
    fn spline(&self) -> RisePubkey { r(self.b[6].key) }
    fn global_vault(&self) -> RisePubkey { r(self.b[7].key) }
    fn trader_token(&self) -> RisePubkey { r(self.b[8].key) }
    fn canonical_mint(&self) -> RisePubkey { r(self.b[9].key) }
    fn usdc_mint(&self) -> RisePubkey { r(self.b[13].key) }
    fn usdc_buffer(&self) -> RisePubkey { r(self.b[14].key) }
    fn gti_keys(&self) -> Vec<RisePubkey> {
        self.b[16..16 + self.gti].iter().map(|a| r(a.key)).collect()
    }
    fn atb_keys(&self) -> Vec<RisePubkey> {
        self.b[16 + self.gti..16 + self.gti + self.atb].iter().map(|a| r(a.key)).collect()
    }
}

fn r(p: &Pubkey) -> RisePubkey {
    RisePubkey::new_from_array(p.to_bytes())
}

fn convert(ix: phoenix_rise_ix::types::Instruction) -> Instruction {
    Instruction {
        program_id: Pubkey::new_from_array(ix.program_id.to_bytes()),
        accounts: ix
            .accounts
            .into_iter()
            .map(|m| AccountMeta { pubkey: Pubkey::new_from_array(m.pubkey.to_bytes()), is_signer: m.is_signer, is_writable: m.is_writable })
            .collect(),
        data: ix.data,
    }
}

fn ix_err(_: phoenix_rise_ix::error::PhoenixIxError) -> Error {
    error!(CarreraError::InvalidArgument)
}

/// USDC (`usdc_buffer`) → Ember wrap → Phoenix deposit into the trader account.
pub fn deposit_collateral(ctx: &VenueCtx, amount: u64) -> Result<()> {
    if super::MOCK || amount == 0 {
        return Ok(());
    }
    let p = P::load(ctx)?;
    let trader = r(ctx.vault.key);
    let ember = EmberDepositParams::builder()
        .trader(trader)
        .usdc_mint(p.usdc_mint())
        .canonical_mint(p.canonical_mint())
        .trader_usdc_account(p.usdc_buffer())
        .trader_phoenix_account(p.trader_token())
        .amount(amount)
        .build()
        .map_err(ix_err)?;
    ctx.invoke(convert(phoenix_rise_ix::ember_deposit::create_ember_deposit_ix(ember).map_err(ix_err)?), p.b)?;
    let dep = DepositFundsParams::builder()
        .trader(trader)
        .trader_account(p.trader_account())
        .canonical_mint(p.canonical_mint())
        .global_vault(p.global_vault())
        .trader_token_account(p.trader_token())
        .global_trader_index(p.gti_keys())
        .active_trader_buffer(p.atb_keys())
        .amount(amount)
        .build()
        .map_err(ix_err)?;
    ctx.invoke(convert(phoenix_rise_ix::deposit_funds::create_deposit_funds_ix(dep).map_err(ix_err)?), p.b)
}

/// Phoenix withdraw → Ember unwrap → USDC back into `usdc_buffer`.
pub fn withdraw_collateral(ctx: &VenueCtx, amount: u64) -> Result<()> {
    if super::MOCK || amount == 0 {
        return Ok(());
    }
    let p = P::load(ctx)?;
    let trader = r(ctx.vault.key);
    let wd = WithdrawFundsParams::builder()
        .trader(trader)
        .trader_account(p.trader_account())
        .perp_asset_map(p.perp_asset_map())
        .global_vault(p.global_vault())
        .trader_token_account(p.trader_token())
        .global_trader_index(p.gti_keys())
        .active_trader_buffer(p.atb_keys())
        .amount(amount)
        .build()
        .map_err(ix_err)?;
    ctx.invoke(convert(phoenix_rise_ix::withdraw_funds::create_withdraw_funds_ix(wd).map_err(ix_err)?), p.b)?;
    let before = token_amount(&p.b[14])?;
    let ember = EmberWithdrawParams::builder()
        .trader(trader)
        .usdc_mint(p.usdc_mint())
        .canonical_mint(p.canonical_mint())
        .trader_usdc_account(p.usdc_buffer())
        .trader_phoenix_account(p.trader_token())
        .amount(Some(amount))
        .build()
        .map_err(ix_err)?;
    ctx.invoke(convert(phoenix_rise_ix::ember_withdraw::create_ember_withdraw_ix(ember).map_err(ix_err)?), p.b)?;
    let after = token_amount(&p.b[14])?;
    require!(after.saturating_sub(before) >= amount, CarreraError::VenueCpiFailed);
    Ok(())
}

fn order(ctx: &VenueCtx, side: Side, qty: u64, flags: OrderFlags) -> Result<u64> {
    let p = P::load(ctx)?;
    let lot = ctx.data.base_lot_size;
    require!(lot > 0, CarreraError::InvalidArgument);
    let lots = qty / lot;
    require!(lots > 0, CarreraError::InvalidArgument);
    let mut b = MarketOrderParams::builder()
        .trader(r(ctx.vault.key))
        .trader_account(p.trader_account())
        .perp_asset_map(p.perp_asset_map())
        .orderbook(p.orderbook())
        .spline_collection(p.spline())
        .global_trader_index(p.gti_keys())
        .active_trader_buffer(p.atb_keys())
        .side(side)
        .num_base_lots(lots)
        .min_base_lots_to_fill(lots)
        .self_trade_behavior(SelfTradeBehavior::Abort)
        .order_flags(flags)
        .client_order_id(ctx.data.client_order_id as u128);
    if ctx.data.price_in_ticks > 0 {
        b = b.price_in_ticks(ctx.data.price_in_ticks);
    }
    if ctx.data.last_valid_slot > 0 {
        b = b.last_valid_slot(ctx.data.last_valid_slot);
    }
    let params = b.build().map_err(ix_err)?;
    ctx.invoke(convert(phoenix_rise_ix::market_order::create_place_market_order_ix(params).map_err(ix_err)?), p.b)?;
    // All-or-nothing fill: Phoenix rejected the order unless every lot filled.
    Ok(lots * lot)
}

/// Open a short of `qty` stock base units (rounded down to whole base lots).
/// Returns the filled quantity in base units.
pub fn open_short(ctx: &VenueCtx, _vault: &OverlayVault, qty: u64) -> Result<u64> {
    if super::MOCK {
        return Ok(qty);
    }
    order(ctx, Side::Ask, qty, OrderFlags::None)
}

/// Close `qty` of the short (reduce-only bid). Returns (filled qty, realised PnL in USDC).
/// Realised PnL is not read on-chain (D6); it shows up in the keeper-supplied equity.
pub fn close_short(ctx: &VenueCtx, _vault: &OverlayVault, qty: u64) -> Result<(u64, i64)> {
    if super::MOCK {
        return Ok((qty, 0));
    }
    Ok((order(ctx, Side::Bid, qty, OrderFlags::ReduceOnly)?, 0))
}

/// Subaccount equity: keeper-supplied in non-mock builds (D6), cached field in mock builds.
pub fn read_equity(ctx: &VenueCtx, vault: &OverlayVault) -> Result<u64> {
    if super::MOCK {
        return Ok(vault.phoenix_equity_usdc);
    }
    Ok(ctx.data.phoenix_equity_usdc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builders_produce_phoenix_instructions() {
        let k = || RisePubkey::new_unique();
        let params = MarketOrderParams::builder()
            .trader(k()).trader_account(k()).perp_asset_map(k()).orderbook(k()).spline_collection(k())
            .global_trader_index(vec![k()]).active_trader_buffer(vec![k()])
            .side(Side::Ask).num_base_lots(10).min_base_lots_to_fill(10)
            .order_flags(OrderFlags::None).build().unwrap();
        let ix = convert(phoenix_rise_ix::market_order::create_place_market_order_ix(params).unwrap());
        assert_eq!(ix.program_id, PHOENIX_PROGRAM_ID);
        assert_eq!(ix.accounts[0].pubkey, PHOENIX_PROGRAM_ID);
        assert!(ix.accounts[3].is_signer, "trader signs");
        assert_eq!(ix.accounts.len(), 2 + 4 + 2 + 2);
        assert_eq!(ix.data.len() > 8, true);
        let ember = EmberDepositParams::builder()
            .trader(k()).usdc_mint(k()).canonical_mint(k()).trader_usdc_account(k()).trader_phoenix_account(k()).amount(5)
            .build().unwrap();
        let ix = convert(phoenix_rise_ix::ember_deposit::create_ember_deposit_ix(ember).unwrap());
        assert_eq!(ix.program_id, EMBER_PROGRAM_ID);
        assert_eq!(ix.accounts.len(), 8);
    }
}
