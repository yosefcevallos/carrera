//! Kamino Lend (klend v1.25) CPIs and on-chain reads for the xStocks market.
//!
//! The vault PDA owns one obligation (tag 0, id 0) in the xStocks market. Its
//! xStock reserve holds the depositor stock plus the basis spot leg as
//! collateral; the USDC reserve is where the primary and secondary loans are
//! drawn, and where Parked USDC is supplied (cTokens held by the vault).
//!
//! # Kamino block (24 accounts, `VenueData::blocks & BLOCK_KAMINO`)
//! ```text
//!  0 klend_program                     KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD
//!  1 lending_market                    5wJeMrUYECGq41fxRESKALVcHnNX26TAWy4W98yULsua
//!  2 lending_market_authority          PDA ["lma", lending_market] of klend
//!  3 obligation                        PDA [0, 0, vault, lending_market, system, system] of klend
//!  4 stock_reserve                     the vault's xStock reserve
//!  5 stock_reserve_liquidity_supply    reserve.liquidity.supply_vault
//!  6 stock_reserve_collateral_mint     reserve.collateral.mint_pubkey
//!  7 stock_reserve_collateral_supply   reserve.collateral.supply_vault
//!  8 xstock_mint
//!  9 usdc_reserve                      97zoywd8mPZsGTg8q1wdD2Wgkdrs2tqusp1Qqcxbyj7E
//! 10 usdc_reserve_liquidity_supply
//! 11 usdc_reserve_fee_receiver         reserve.liquidity.fee_vault
//! 12 usdc_reserve_collateral_mint
//! 13 vault_usdc_ctoken                 vault-owned token account of the USDC cToken mint
//! 14 usdc_mint
//! 15 scope_prices                      reserve.config.token_info.scope_configuration.price_feed
//! 16 farms_program                     FarmsPZpWu9i7Kky8tPN37rs2TpmMrAZrC7S7vJa91Hr
//! 17 instructions_sysvar               Sysvar1nstructions1111111111111111111111111
//! 18 token_program                     classic SPL Token
//! 19 stock_token_program               Token-2022 for the xStocks
//! 20 stock_custody                     vault PDA ["stock", vault]
//! 21 usdc_buffer                       vault PDA ["usdc", vault]
//! 22 usdc_debt_farm_state              reserve_usdc.farm_debt (klend program id when the reserve has no debt farm)
//! 23 obligation_debt_farm_user_state   PDA ["user", farm_debt, obligation] of Farms (klend program id when none)
//! ```
//! The xStocks reserves have no farms, so deposit/withdraw get program-id placeholders
//! for the optional farm accounts. The USDC reserve has a **debt farm** on mainnet, so
//! borrow/repay carry the obligation's farm user state (created by `init_obligation`
//! through `init_obligation_farms_for_reserve`, mode 1) and the reserve's farm state.

use super::{meta, token_amount, VenueCtx};
use crate::errors::CarreraError;
use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::Instruction;

pub const KLEND_PROGRAM_ID: Pubkey = pubkey!("KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD");
pub const FARMS_PROGRAM_ID: Pubkey = pubkey!("FarmsPZpWu9i7Kky8tPN37rs2TpmMrAZrC7S7vJa91Hr");

// Anchor discriminators, sha256("global:<name>")[..8]; verified in `tests::discriminators`.
const D_INIT_USER_METADATA: [u8; 8] = [117, 169, 176, 69, 197, 23, 15, 162];
const D_INIT_OBLIGATION: [u8; 8] = [251, 10, 231, 76, 27, 11, 159, 96];
const D_REFRESH_RESERVE: [u8; 8] = [2, 218, 138, 235, 79, 201, 25, 102];
const D_REFRESH_OBLIGATION: [u8; 8] = [33, 132, 147, 228, 151, 192, 72, 89];
const D_DEPOSIT_COLL_V2: [u8; 8] = [216, 224, 191, 27, 204, 151, 102, 175];
const D_WITHDRAW_COLL_V2: [u8; 8] = [235, 52, 119, 152, 149, 197, 20, 7];
const D_BORROW_V2: [u8; 8] = [161, 128, 143, 245, 171, 199, 194, 6];
const D_REPAY_V2: [u8; 8] = [116, 174, 213, 76, 180, 53, 210, 144];
const D_DEPOSIT_LIQUIDITY: [u8; 8] = [169, 201, 30, 126, 6, 205, 102, 68];
const D_REDEEM_COLLATERAL: [u8; 8] = [234, 117, 181, 125, 185, 142, 220, 29];
const D_INIT_OBLIGATION_FARMS: [u8; 8] = [136, 63, 15, 186, 211, 152, 168, 164];

/// Byte offsets inside a `Reserve` account (after the 8-byte discriminator), klend
/// v1.25 sequential layout. Validated against the live USDC and TSLAx reserves in
/// `program/tests/fixtures/kamino_layout.py` and in `tests::reserve_fixture_parses`.
pub mod reserve {
    pub const LEN: usize = 8616;
    pub const LENDING_MARKET: usize = 24;
    pub const FARM_COLLATERAL: usize = 56;
    pub const FARM_DEBT: usize = 88;
    pub const LIQ_MINT: usize = 120;
    pub const LIQ_SUPPLY_VAULT: usize = 152;
    pub const LIQ_FEE_VAULT: usize = 184;
    pub const LIQ_AVAILABLE: usize = 216;
    pub const LIQ_BORROWED_SF: usize = 224;
    pub const LIQ_MARKET_PRICE_SF: usize = 240;
    pub const LIQ_PRICE_TS: usize = 256;
    pub const LIQ_DECIMALS: usize = 264;
    pub const LIQ_ACC_PROTOCOL_FEES_SF: usize = 320;
    pub const LIQ_ACC_REFERRER_FEES_SF: usize = 336;
    pub const LIQ_PENDING_REFERRER_FEES_SF: usize = 352;
    pub const LIQ_TOKEN_PROGRAM: usize = 400;
    pub const COLL_MINT: usize = 2552;
    pub const COLL_MINT_TOTAL_SUPPLY: usize = 2584;
    pub const COLL_SUPPLY_VAULT: usize = 2592;
    pub const CFG_PROTOCOL_TAKE_RATE_PCT: usize = 4862;
    pub const CFG_BORROW_RATE_CURVE: usize = 4912; // 11 × (u32 utilization_bps, u32 rate_bps)
    pub const CFG_SCOPE_PRICE_FEED: usize = 5104;
}

/// Maximum age of the reserve's market price accepted by `read_price`.
pub const MAX_PRICE_AGE_SECS: i64 = 600;
const SF: u128 = 1 << 60;

// ------------------------------------------------------------ account block

struct K<'a, 'info> {
    b: &'a [AccountInfo<'info>],
}

impl<'a, 'info> K<'a, 'info> {
    // klend declares the obligation `owner` signer as `mut` on every handler, so the
    // vault is always passed as a writable signer (its data is never touched by klend).
    fn program(&self) -> &AccountInfo<'info> { &self.b[0] }
    fn market(&self) -> &AccountInfo<'info> { &self.b[1] }
    fn market_auth(&self) -> &AccountInfo<'info> { &self.b[2] }
    fn obligation(&self) -> &AccountInfo<'info> { &self.b[3] }
    fn stock_reserve(&self) -> &AccountInfo<'info> { &self.b[4] }
    fn stock_liq_supply(&self) -> &AccountInfo<'info> { &self.b[5] }
    fn stock_coll_mint(&self) -> &AccountInfo<'info> { &self.b[6] }
    fn stock_coll_supply(&self) -> &AccountInfo<'info> { &self.b[7] }
    fn xstock_mint(&self) -> &AccountInfo<'info> { &self.b[8] }
    fn usdc_reserve(&self) -> &AccountInfo<'info> { &self.b[9] }
    fn usdc_liq_supply(&self) -> &AccountInfo<'info> { &self.b[10] }
    fn usdc_fee_receiver(&self) -> &AccountInfo<'info> { &self.b[11] }
    fn usdc_coll_mint(&self) -> &AccountInfo<'info> { &self.b[12] }
    fn vault_usdc_ctoken(&self) -> &AccountInfo<'info> { &self.b[13] }
    fn usdc_mint(&self) -> &AccountInfo<'info> { &self.b[14] }
    fn scope_prices(&self) -> &AccountInfo<'info> { &self.b[15] }
    fn farms_program(&self) -> &AccountInfo<'info> { &self.b[16] }
    fn ix_sysvar(&self) -> &AccountInfo<'info> { &self.b[17] }
    fn token_program(&self) -> &AccountInfo<'info> { &self.b[18] }
    fn stock_token_program(&self) -> &AccountInfo<'info> { &self.b[19] }
    fn stock_custody(&self) -> &AccountInfo<'info> { &self.b[20] }
    fn usdc_buffer(&self) -> &AccountInfo<'info> { &self.b[21] }
    fn usdc_debt_farm(&self) -> &AccountInfo<'info> { &self.b[22] }
    fn obligation_debt_farm_user(&self) -> &AccountInfo<'info> { &self.b[23] }

    /// The USDC reserve's debt farm, if it has one.
    fn usdc_debt_farm_key(&self) -> Result<Option<Pubkey>> {
        let ur = self.usdc_reserve().try_borrow_data()?;
        let f = pk(&ur[8..], reserve::FARM_DEBT);
        Ok(if f == Pubkey::default() { None } else { Some(f) })
    }

    fn validate(&self, ctx: &VenueCtx<'a, 'info>) -> Result<()> {
        require_keys_eq!(*self.program().key, KLEND_PROGRAM_ID, CarreraError::VenueAccountsMismatch);
        require_keys_eq!(*self.farms_program().key, FARMS_PROGRAM_ID, CarreraError::VenueAccountsMismatch);
        require_keys_eq!(*self.xstock_mint().key, ctx.xstock_mint, CarreraError::VenueAccountsMismatch);
        require_keys_eq!(
            *self.ix_sysvar().key,
            anchor_lang::solana_program::sysvar::instructions::ID,
            CarreraError::VenueAccountsMismatch
        );
        // Reserves must belong to this market and to the mints we expect.
        let sr_full = self.stock_reserve().try_borrow_data()?;
        let ur_full = self.usdc_reserve().try_borrow_data()?;
        require!(sr_full.len() == reserve::LEN + 8 && ur_full.len() == reserve::LEN + 8, CarreraError::VenueAccountsMismatch);
        // Offsets are relative to the data after the 8-byte discriminator.
        let (sr, ur) = (&sr_full[8..], &ur_full[8..]);
        require!(
            pk(&sr, reserve::LENDING_MARKET) == *self.market().key && pk(&ur, reserve::LENDING_MARKET) == *self.market().key,
            CarreraError::VenueAccountsMismatch
        );
        require!(pk(&sr, reserve::LIQ_MINT) == ctx.xstock_mint, CarreraError::VenueAccountsMismatch);
        require!(pk(&ur, reserve::LIQ_MINT) == *self.usdc_mint().key, CarreraError::VenueAccountsMismatch);
        require!(pk(&sr, reserve::LIQ_SUPPLY_VAULT) == *self.stock_liq_supply().key, CarreraError::VenueAccountsMismatch);
        require!(pk(&sr, reserve::COLL_MINT) == *self.stock_coll_mint().key, CarreraError::VenueAccountsMismatch);
        require!(pk(&sr, reserve::COLL_SUPPLY_VAULT) == *self.stock_coll_supply().key, CarreraError::VenueAccountsMismatch);
        require!(pk(&ur, reserve::LIQ_SUPPLY_VAULT) == *self.usdc_liq_supply().key, CarreraError::VenueAccountsMismatch);
        require!(pk(&ur, reserve::LIQ_FEE_VAULT) == *self.usdc_fee_receiver().key, CarreraError::VenueAccountsMismatch);
        require!(pk(&ur, reserve::COLL_MINT) == *self.usdc_coll_mint().key, CarreraError::VenueAccountsMismatch);
        require!(pk(&sr, reserve::CFG_SCOPE_PRICE_FEED) == *self.scope_prices().key, CarreraError::VenueAccountsMismatch);
        require!(pk(&sr, reserve::LIQ_TOKEN_PROGRAM) == *self.stock_token_program().key, CarreraError::VenueAccountsMismatch);
        require!(pk(&ur, reserve::LIQ_TOKEN_PROGRAM) == *self.token_program().key, CarreraError::VenueAccountsMismatch);
        // Vault-owned token accounts.
        let (custody, _) = Pubkey::find_program_address(&[b"stock", ctx.vault.key.as_ref()], &crate::ID);
        let (buffer, _) = Pubkey::find_program_address(&[b"usdc", ctx.vault.key.as_ref()], &crate::ID);
        require_keys_eq!(*self.stock_custody().key, custody, CarreraError::VenueAccountsMismatch);
        require_keys_eq!(*self.usdc_buffer().key, buffer, CarreraError::VenueAccountsMismatch);
        require!(token_owner(self.vault_usdc_ctoken())? == *ctx.vault.key, CarreraError::VenueAccountsMismatch);
        // Derived klend PDAs.
        let (auth, _) = Pubkey::find_program_address(&[b"lma", self.market().key.as_ref()], &KLEND_PROGRAM_ID);
        require_keys_eq!(*self.market_auth().key, auth, CarreraError::VenueAccountsMismatch);
        require_keys_eq!(*self.obligation().key, obligation_address(ctx.vault.key, self.market().key), CarreraError::VenueAccountsMismatch);
        // USDC debt farm accounts (or placeholders when the reserve has none).
        match self.usdc_debt_farm_key()? {
            Some(farm) => {
                require_keys_eq!(*self.usdc_debt_farm().key, farm, CarreraError::VenueAccountsMismatch);
                require_keys_eq!(
                    *self.obligation_debt_farm_user().key,
                    farm_user_state_address(&farm, self.obligation().key),
                    CarreraError::VenueAccountsMismatch
                );
            }
            None => {
                require_keys_eq!(*self.usdc_debt_farm().key, KLEND_PROGRAM_ID, CarreraError::VenueAccountsMismatch);
                require_keys_eq!(*self.obligation_debt_farm_user().key, KLEND_PROGRAM_ID, CarreraError::VenueAccountsMismatch);
            }
        }
        Ok(())
    }

    fn refresh_reserves(&self, ctx: &VenueCtx<'a, 'info>) -> Result<()> {
        for r in [self.stock_reserve(), self.usdc_reserve()] {
            let ix = Instruction {
                program_id: KLEND_PROGRAM_ID,
                accounts: vec![
                    meta(*r.key, true, false),
                    meta(*self.market().key, false, false),
                    meta(KLEND_PROGRAM_ID, false, false), // pyth_oracle: none
                    meta(KLEND_PROGRAM_ID, false, false), // switchboard_price_oracle: none
                    meta(KLEND_PROGRAM_ID, false, false), // switchboard_twap_oracle: none
                    meta(*self.scope_prices().key, false, false),
                ],
                data: D_REFRESH_RESERVE.to_vec(),
            };
            ctx.invoke(ix, self.b)?;
        }
        Ok(())
    }

    /// Refresh the obligation. Kamino wants exactly the reserves of the obligation's
    /// active deposits, then its active borrows, as remaining accounts. The vault
    /// only ever deposits the stock reserve and borrows the USDC reserve, so the
    /// obligation bytes are scanned for those two keys.
    fn refresh_obligation(&self, ctx: &VenueCtx<'a, 'info>) -> Result<()> {
        let (has_deposit, has_borrow) = {
            let d = self.obligation().try_borrow_data()?;
            (
                obligation_has_deposit(&d, self.stock_reserve().key),
                obligation_has_borrow(&d, self.usdc_reserve().key),
            )
        };
        let mut accounts = vec![
            meta(*self.market().key, false, false),
            meta(*self.obligation().key, true, false),
        ];
        // klend loads these with `get_mut`, so they must be writable.
        if has_deposit {
            accounts.push(meta(*self.stock_reserve().key, true, false));
        }
        if has_borrow {
            accounts.push(meta(*self.usdc_reserve().key, true, false));
        }
        let ix = Instruction { program_id: KLEND_PROGRAM_ID, accounts, data: D_REFRESH_OBLIGATION.to_vec() };
        ctx.invoke(ix, self.b)
    }

    fn refresh_all(&self, ctx: &VenueCtx<'a, 'info>) -> Result<()> {
        self.refresh_reserves(ctx)?;
        self.refresh_obligation(ctx)
    }

    /// Farm accounts for the stock reserve (none on the xStocks reserves).
    fn farms_tail(&self) -> [anchor_lang::solana_program::instruction::AccountMeta; 3] {
        [
            meta(KLEND_PROGRAM_ID, false, false), // obligation_farm_user_state: none
            meta(KLEND_PROGRAM_ID, false, false), // reserve_farm_state: none
            meta(FARMS_PROGRAM_ID, false, false),
        ]
    }

    /// Farm accounts for the USDC reserve: (obligation_farm_user_state, reserve_farm_state).
    fn usdc_farm_metas(&self) -> [anchor_lang::solana_program::instruction::AccountMeta; 2] {
        let has = *self.usdc_debt_farm().key != KLEND_PROGRAM_ID;
        [
            meta(*self.obligation_debt_farm_user().key, has, false),
            meta(*self.usdc_debt_farm().key, has, false),
        ]
    }
}

fn pk(d: &[u8], off: usize) -> Pubkey {
    Pubkey::new_from_array(d[off..off + 32].try_into().unwrap())
}
fn u64_at(d: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(d[off..off + 8].try_into().unwrap())
}
fn u128_at(d: &[u8], off: usize) -> u128 {
    u128::from_le_bytes(d[off..off + 16].try_into().unwrap())
}
fn token_owner(ai: &AccountInfo) -> Result<Pubkey> {
    let d = ai.try_borrow_data()?;
    require!(d.len() >= 72, CarreraError::VenueAccountsMismatch);
    Ok(pk(&d, 32))
}

/// Obligation layout (klend v1.25, after the 8-byte discriminator): tag 8, last_update 16,
/// lending_market 32, owner 32, then `deposits: [ObligationCollateral; 8]` at 88 with a
/// 136-byte stride (reserve pubkey first), then `lowest_reserve_deposit_liquidation_ltv` 8,
/// `deposited_value_sf` 16, then `borrows: [ObligationLiquidity; 5]` from 1200 (reserve
/// pubkey first). `OBLIGATION_SIZE = 3336`.
pub mod obligation {
    pub const LEN: usize = 3336;
    pub const DEPOSITS: usize = 88;
    pub const DEPOSIT_STRIDE: usize = 136;
    pub const DEPOSITS_N: usize = 8;
    pub const BORROWS: usize = 1200;
    /// Upper bound of the borrows region scanned for a reserve key (5 entries, ≤ 320 bytes each).
    pub const BORROWS_END: usize = 1200 + 5 * 320;
}

/// True when `reserve` is one of the obligation's active deposit reserves.
pub fn obligation_has_deposit(account_data: &[u8], reserve: &Pubkey) -> bool {
    if account_data.len() < obligation::LEN {
        return false;
    }
    let d = &account_data[8..];
    (0..obligation::DEPOSITS_N).any(|i| {
        let o = obligation::DEPOSITS + i * obligation::DEPOSIT_STRIDE;
        &d[o..o + 32] == reserve.as_ref()
    })
}

/// cTokens the obligation holds for `reserve` (`deposited_amount`, right after the reserve key).
pub fn obligation_deposited(account_data: &[u8], reserve: &Pubkey) -> Option<u64> {
    if account_data.len() < obligation::LEN {
        return None;
    }
    let d = &account_data[8..];
    (0..obligation::DEPOSITS_N).find_map(|i| {
        let o = obligation::DEPOSITS + i * obligation::DEPOSIT_STRIDE;
        (&d[o..o + 32] == reserve.as_ref()).then(|| u64_at(d, o + 32))
    })
}

/// True when `reserve` appears (8-byte aligned) in the obligation's borrows region.
pub fn obligation_has_borrow(account_data: &[u8], reserve: &Pubkey) -> bool {
    if account_data.len() < obligation::LEN {
        return false;
    }
    let d = &account_data[8..];
    let end = obligation::BORROWS_END.min(d.len() - 32);
    (obligation::BORROWS..=end).step_by(8).any(|o| &d[o..o + 32] == reserve.as_ref())
}

/// The vault's obligation address: tag 0, id 0, no seed accounts.
pub fn obligation_address(vault: &Pubkey, market: &Pubkey) -> Pubkey {
    let sys = anchor_lang::solana_program::system_program::ID;
    Pubkey::find_program_address(&[&[0u8], &[0u8], vault.as_ref(), market.as_ref(), sys.as_ref(), sys.as_ref()], &KLEND_PROGRAM_ID).0
}
pub fn user_metadata_address(vault: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"user_meta", vault.as_ref()], &KLEND_PROGRAM_ID).0
}

fn with_amount(disc: [u8; 8], amount: u64) -> Vec<u8> {
    let mut d = disc.to_vec();
    d.extend_from_slice(&amount.to_le_bytes());
    d
}

// ------------------------------------------------------------ reserve maths

/// cToken amount worth at least `liquidity` in the reserve (rounded up). Uses
/// Kamino's collateral exchange rate: mint_total_supply / (available + borrowed − fees).
pub fn liquidity_to_collateral(reserve_data: &[u8], liquidity: u64) -> Option<u64> {
    let d = &reserve_data[8..];
    let mint_total_supply = u64_at(d, reserve::COLL_MINT_TOTAL_SUPPLY) as u128;
    let available = u64_at(d, reserve::LIQ_AVAILABLE) as u128;
    let borrowed = u128_at(d, reserve::LIQ_BORROWED_SF) / SF;
    let fees = (u128_at(d, reserve::LIQ_ACC_PROTOCOL_FEES_SF)
        + u128_at(d, reserve::LIQ_ACC_REFERRER_FEES_SF)
        + u128_at(d, reserve::LIQ_PENDING_REFERRER_FEES_SF))
        / SF;
    let total_liquidity = (available + borrowed).checked_sub(fees)?;
    if total_liquidity == 0 || mint_total_supply == 0 {
        return Some(liquidity); // 1:1 at genesis
    }
    let c = (liquidity as u128).checked_mul(mint_total_supply)?.div_ceil(total_liquidity);
    u64::try_from(c).ok()
}

/// (borrow APR bps, supply APR bps) from the reserve's curve and utilisation.
/// Supply = borrow × utilisation × (1 − protocol_take_rate). APR, not compounded APY.
pub fn rates_from_reserve(reserve_data: &[u8]) -> Option<(u32, u32)> {
    let d = &reserve_data[8..];
    let available = u64_at(d, reserve::LIQ_AVAILABLE) as u128;
    let borrowed = u128_at(d, reserve::LIQ_BORROWED_SF) / SF;
    let total = available + borrowed;
    let util_bps: u32 = if total == 0 { 0 } else { (borrowed * 10_000 / total) as u32 };
    let c = reserve::CFG_BORROW_RATE_CURVE;
    let point = |i: usize| -> (u32, u32) {
        let o = c + 8 * i;
        (
            u32::from_le_bytes(d[o..o + 4].try_into().unwrap()),
            u32::from_le_bytes(d[o + 4..o + 8].try_into().unwrap()),
        )
    };
    let mut borrow_bps = point(10).1;
    for i in 0..10 {
        let (u0, r0) = point(i);
        let (u1, r1) = point(i + 1);
        if util_bps <= u1 {
            borrow_bps = if u1 == u0 {
                r0
            } else {
                r0 + ((r1 as i64 - r0 as i64) * (util_bps as i64 - u0 as i64) / (u1 as i64 - u0 as i64)) as u32
            };
            break;
        }
    }
    let take = d[reserve::CFG_PROTOCOL_TAKE_RATE_PCT] as u128;
    let supply_bps = (borrow_bps as u128 * util_bps as u128 * (100 - take) / 10_000 / 100) as u32;
    Some((borrow_bps, supply_bps))
}

/// USD × 1e6 per whole token from `liquidity.market_price_sf`, with its timestamp.
pub fn price_from_reserve(reserve_data: &[u8]) -> Option<(u64, i64)> {
    let d = &reserve_data[8..];
    let sf = u128_at(d, reserve::LIQ_MARKET_PRICE_SF);
    let price_e6 = u64::try_from(sf.checked_mul(1_000_000)? >> 60).ok()?;
    let ts = u64_at(d, reserve::LIQ_PRICE_TS) as i64;
    Some((price_e6, ts))
}

// ------------------------------------------------------------ adapters

/// Move `qty` xStock from `stock_custody` into the obligation as collateral.
pub fn deposit_collateral(ctx: &VenueCtx, qty: u64) -> Result<()> {
    if super::MOCK || qty == 0 {
        return Ok(());
    }
    let k = K { b: ctx.kamino()? };
    k.validate(ctx)?;
    k.refresh_all(ctx)?;
    let mut accounts = vec![
        meta(*ctx.vault.key, true, true),
        meta(*k.obligation().key, true, false),
        meta(*k.market().key, false, false),
        meta(*k.market_auth().key, false, false),
        meta(*k.stock_reserve().key, true, false),
        meta(*k.xstock_mint().key, false, false),
        meta(*k.stock_liq_supply().key, true, false),
        meta(*k.stock_coll_mint().key, true, false),
        meta(*k.stock_coll_supply().key, true, false),
        meta(*k.stock_custody().key, true, false),
        meta(KLEND_PROGRAM_ID, false, false), // placeholder_user_destination_collateral
        meta(*k.token_program().key, false, false),
        meta(*k.stock_token_program().key, false, false),
        meta(*k.ix_sysvar().key, false, false),
    ];
    accounts.extend(k.farms_tail());
    ctx.invoke(Instruction { program_id: KLEND_PROGRAM_ID, accounts, data: with_amount(D_DEPOSIT_COLL_V2, qty) }, k.b)
}

/// Withdraw at least `qty` xStock from the obligation into `stock_custody`.
/// Rounding slack on a full collateral withdrawal: Kamino floors both the cTokens minted on
/// deposit and the liquidity redeemed on withdrawal, so emptying the obligation can return a
/// few base units (1e-8 stock) less than was deposited.
pub const STOCK_ROUNDING_TOL: u64 = 16;

/// Withdraw `qty` stock from the obligation into custody (stock already sitting in custody counts
/// first). Returns the amount of `qty` now available in custody: `qty` in all but the
/// full-withdrawal case, where it may fall short by rounding dust (≤ `STOCK_ROUNDING_TOL` or 1 bp).
pub fn withdraw_collateral(ctx: &VenueCtx, qty: u64) -> Result<u64> {
    if super::MOCK || qty == 0 {
        return Ok(qty);
    }
    let k = K { b: ctx.kamino()? };
    k.validate(ctx)?;
    // Stock still in custody (never synced into the obligation, e.g. right after the mainnet
    // upgrade, or an obligation that does not exist yet) is paid out from custody directly.
    let in_custody = token_amount(k.stock_custody())?;
    if in_custody >= qty {
        return Ok(qty);
    }
    require!(k.obligation().data_len() > 8, CarreraError::VenueAccountsMissing);
    let need = qty - in_custody;
    k.refresh_all(ctx)?;
    let coll = liquidity_to_collateral(&k.stock_reserve().try_borrow_data()?, need).ok_or(CarreraError::MathOverflow)?;
    // One extra cToken covers the floor on redemption; a full withdrawal takes everything held.
    let held = obligation_deposited(&k.obligation().try_borrow_data()?, k.stock_reserve().key).unwrap_or(0);
    let coll = coll.saturating_add(1).min(held);
    require!(coll > 0, CarreraError::VenueCpiFailed);
    let before = token_amount(k.stock_custody())?;
    let mut accounts = vec![
        meta(*ctx.vault.key, true, true),
        meta(*k.obligation().key, true, false),
        meta(*k.market().key, false, false),
        meta(*k.market_auth().key, false, false),
        meta(*k.stock_reserve().key, true, false),
        meta(*k.xstock_mint().key, false, false),
        meta(*k.stock_coll_supply().key, true, false),
        meta(*k.stock_coll_mint().key, true, false),
        meta(*k.stock_liq_supply().key, true, false),
        meta(*k.stock_custody().key, true, false),
        meta(KLEND_PROGRAM_ID, false, false), // placeholder_user_destination_collateral
        meta(*k.token_program().key, false, false),
        meta(*k.stock_token_program().key, false, false),
        meta(*k.ix_sysvar().key, false, false),
    ];
    accounts.extend(k.farms_tail());
    ctx.invoke(Instruction { program_id: KLEND_PROGRAM_ID, accounts, data: with_amount(D_WITHDRAW_COLL_V2, coll) }, k.b)?;
    let received = token_amount(k.stock_custody())?.saturating_sub(before);
    let tol = STOCK_ROUNDING_TOL.max(need / 10_000);
    require!(received.saturating_add(tol) >= need, CarreraError::VenueCpiFailed);
    Ok(qty.min(in_custody + received))
}

/// Borrow `amount` USDC against the obligation into `usdc_buffer`.
pub fn borrow_usdc(ctx: &VenueCtx, amount: u64) -> Result<()> {
    if super::MOCK || amount == 0 {
        return Ok(());
    }
    let k = K { b: ctx.kamino()? };
    k.validate(ctx)?;
    k.refresh_all(ctx)?;
    let before = token_amount(k.usdc_buffer())?;
    let mut accounts = vec![
        meta(*ctx.vault.key, true, true),
        meta(*k.obligation().key, true, false),
        meta(*k.market().key, false, false),
        meta(*k.market_auth().key, false, false),
        meta(*k.usdc_reserve().key, true, false),
        meta(*k.usdc_mint().key, false, false),
        meta(*k.usdc_liq_supply().key, true, false),
        meta(*k.usdc_fee_receiver().key, true, false),
        meta(*k.usdc_buffer().key, true, false),
        meta(KLEND_PROGRAM_ID, false, false), // referrer_token_state: none
        meta(*k.token_program().key, false, false),
        meta(*k.ix_sysvar().key, false, false),
    ];
    accounts.extend(k.usdc_farm_metas());
    accounts.push(meta(FARMS_PROGRAM_ID, false, false));
    ctx.invoke(Instruction { program_id: KLEND_PROGRAM_ID, accounts, data: with_amount(D_BORROW_V2, amount) }, k.b)?;
    // Kamino's origination fee comes out of the borrowed amount; the vault books the gross.
    let after = token_amount(k.usdc_buffer())?;
    require!(after > before, CarreraError::VenueCpiFailed);
    Ok(())
}

/// Repay `amount` USDC from `usdc_buffer`.
pub fn repay_usdc(ctx: &VenueCtx, amount: u64) -> Result<()> {
    if super::MOCK || amount == 0 {
        return Ok(());
    }
    let k = K { b: ctx.kamino()? };
    k.validate(ctx)?;
    k.refresh_all(ctx)?;
    let mut accounts = vec![
        meta(*ctx.vault.key, true, true),
        meta(*k.obligation().key, true, false),
        meta(*k.market().key, false, false),
        meta(*k.usdc_reserve().key, true, false),
        meta(*k.usdc_mint().key, false, false),
        meta(*k.usdc_liq_supply().key, true, false),
        meta(*k.usdc_buffer().key, true, false),
        meta(*k.token_program().key, false, false),
        meta(*k.ix_sysvar().key, false, false),
    ];
    accounts.extend(k.usdc_farm_metas());
    accounts.push(meta(*k.market_auth().key, false, false));
    accounts.push(meta(FARMS_PROGRAM_ID, false, false));
    ctx.invoke(Instruction { program_id: KLEND_PROGRAM_ID, accounts, data: with_amount(D_REPAY_V2, amount) }, k.b)
}

/// Repay the whole USDC loan (Kamino's `u64::MAX` convention settles the accrued
/// debt exactly, which the vault's cached debt cannot know). `usdc_buffer` must hold
/// at least the accrued debt; the keeper keeps a small cushion there for interest and
/// cToken rounding. Returns the USDC actually taken from the buffer (0 in mock builds).
pub fn repay_usdc_all(ctx: &VenueCtx) -> Result<u64> {
    if super::MOCK {
        return Ok(0);
    }
    let k = K { b: ctx.kamino()? };
    k.validate(ctx)?;
    k.refresh_all(ctx)?;
    let before = token_amount(k.usdc_buffer())?;
    let mut accounts = vec![
        meta(*ctx.vault.key, true, true),
        meta(*k.obligation().key, true, false),
        meta(*k.market().key, false, false),
        meta(*k.usdc_reserve().key, true, false),
        meta(*k.usdc_mint().key, false, false),
        meta(*k.usdc_liq_supply().key, true, false),
        meta(*k.usdc_buffer().key, true, false),
        meta(*k.token_program().key, false, false),
        meta(*k.ix_sysvar().key, false, false),
    ];
    accounts.extend(k.usdc_farm_metas());
    accounts.push(meta(*k.market_auth().key, false, false));
    accounts.push(meta(FARMS_PROGRAM_ID, false, false));
    ctx.invoke(Instruction { program_id: KLEND_PROGRAM_ID, accounts, data: with_amount(D_REPAY_V2, u64::MAX) }, k.b)?;
    let after = token_amount(k.usdc_buffer())?;
    Ok(before.saturating_sub(after))
}

/// Supply `amount` USDC from `usdc_buffer` to the USDC reserve (cTokens to `vault_usdc_ctoken`).
pub fn supply_usdc(ctx: &VenueCtx, amount: u64) -> Result<()> {
    if super::MOCK || amount == 0 {
        return Ok(());
    }
    let k = K { b: ctx.kamino()? };
    k.validate(ctx)?;
    k.refresh_reserves(ctx)?;
    let accounts = vec![
        meta(*ctx.vault.key, true, true),
        meta(*k.usdc_reserve().key, true, false),
        meta(*k.market().key, false, false),
        meta(*k.market_auth().key, false, false),
        meta(*k.usdc_mint().key, false, false),
        meta(*k.usdc_liq_supply().key, true, false),
        meta(*k.usdc_coll_mint().key, true, false),
        meta(*k.usdc_buffer().key, true, false),
        meta(*k.vault_usdc_ctoken().key, true, false),
        meta(*k.token_program().key, false, false),
        meta(*k.token_program().key, false, false),
        meta(*k.ix_sysvar().key, false, false),
    ];
    ctx.invoke(Instruction { program_id: KLEND_PROGRAM_ID, accounts, data: with_amount(D_DEPOSIT_LIQUIDITY, amount) }, k.b)
}

/// Redeem cTokens worth at least `amount` USDC into `usdc_buffer` (capped by the cToken balance).
pub fn withdraw_supplied_usdc(ctx: &VenueCtx, amount: u64) -> Result<()> {
    if super::MOCK || amount == 0 {
        return Ok(());
    }
    let k = K { b: ctx.kamino()? };
    k.validate(ctx)?;
    k.refresh_reserves(ctx)?;
    let want = liquidity_to_collateral(&k.usdc_reserve().try_borrow_data()?, amount).ok_or(CarreraError::MathOverflow)?;
    let held = token_amount(k.vault_usdc_ctoken())?;
    let coll = want.min(held);
    require!(coll > 0, CarreraError::VenueCpiFailed);
    let before = token_amount(k.usdc_buffer())?;
    let accounts = vec![
        meta(*ctx.vault.key, true, true),
        meta(*k.market().key, false, false),
        meta(*k.usdc_reserve().key, true, false),
        meta(*k.market_auth().key, false, false),
        meta(*k.usdc_mint().key, false, false),
        meta(*k.usdc_coll_mint().key, true, false),
        meta(*k.usdc_liq_supply().key, true, false),
        meta(*k.vault_usdc_ctoken().key, true, false),
        meta(*k.usdc_buffer().key, true, false),
        meta(*k.token_program().key, false, false),
        meta(*k.token_program().key, false, false),
        meta(*k.ix_sysvar().key, false, false),
    ];
    ctx.invoke(Instruction { program_id: KLEND_PROGRAM_ID, accounts, data: with_amount(D_REDEEM_COLLATERAL, coll) }, k.b)?;
    let after = token_amount(k.usdc_buffer())?;
    // Interest makes the redeemed liquidity ≥ amount unless the cToken balance capped it.
    require!(after > before, CarreraError::VenueCpiFailed);
    Ok(())
}

/// One-time: create the vault's user metadata and obligation. `payer` funds the rent.
pub fn init_obligation<'a, 'info>(
    ctx: &VenueCtx<'a, 'info>,
    payer: &AccountInfo<'info>,
    system_program: &AccountInfo<'info>,
    rent: &AccountInfo<'info>,
) -> Result<()> {
    if super::MOCK {
        return Ok(());
    }
    let k = K { b: ctx.kamino()? };
    k.validate(ctx)?;
    let user_meta = user_metadata_address(ctx.vault.key);
    let mut pool: Vec<AccountInfo> = k.b.to_vec();
    pool.push(payer.clone());
    pool.push(system_program.clone());
    pool.push(rent.clone());
    // user metadata (idempotent: skip when it already exists)
    let meta_ai = ctx.remaining.iter().find(|a| a.key == &user_meta);
    if meta_ai.map(|a| a.data_is_empty()).unwrap_or(true) {
        let mut data = D_INIT_USER_METADATA.to_vec();
        data.extend_from_slice(&Pubkey::default().to_bytes()); // user_lookup_table: none
        let ix = Instruction {
            program_id: KLEND_PROGRAM_ID,
            accounts: vec![
                meta(*ctx.vault.key, true, true),
                meta(*payer.key, true, true),
                meta(user_meta, true, false),
                meta(KLEND_PROGRAM_ID, false, false), // referrer_user_metadata: none
                meta(*rent.key, false, false),
                meta(*system_program.key, false, false),
            ],
            data,
        };
        let mut p2 = pool.clone();
        if let Some(a) = meta_ai {
            p2.push(a.clone());
        }
        ctx.invoke(ix, &p2)?;
    }
    if k.obligation().data_is_empty() {
        let sys = *system_program.key;
        let mut data = D_INIT_OBLIGATION.to_vec();
        data.extend_from_slice(&[0u8, 0u8]); // InitObligationArgs { tag: 0, id: 0 }
        let ix = Instruction {
            program_id: KLEND_PROGRAM_ID,
            accounts: vec![
                meta(*ctx.vault.key, true, true),
                meta(*payer.key, true, true),
                meta(*k.obligation().key, true, false),
                meta(*k.market().key, false, false),
                meta(sys, false, false), // seed1_account
                meta(sys, false, false), // seed2_account
                meta(user_meta, false, false),
                meta(*rent.key, false, false),
                meta(sys, false, false),
            ],
            data,
        };
        let mut p2 = pool.clone();
        if let Some(a) = meta_ai {
            p2.push(a.clone());
        }
        ctx.invoke(ix, &p2)?;
    }
    // Debt farm user state for the USDC reserve (mode 1 = debt), when the reserve has a farm.
    if k.usdc_debt_farm_key()?.is_some() && k.obligation_debt_farm_user().data_is_empty() {
        let sys = *system_program.key;
        let mut data = D_INIT_OBLIGATION_FARMS.to_vec();
        data.push(1u8);
        let ix = Instruction {
            program_id: KLEND_PROGRAM_ID,
            accounts: vec![
                meta(*payer.key, true, true),
                meta(*ctx.vault.key, false, false), // owner (not a signer here)
                meta(*k.obligation().key, true, false),
                meta(*k.market_auth().key, false, false),
                meta(*k.usdc_reserve().key, true, false),
                meta(*k.usdc_debt_farm().key, true, false),
                meta(*k.obligation_debt_farm_user().key, true, false),
                meta(*k.market().key, false, false),
                meta(FARMS_PROGRAM_ID, false, false),
                meta(*rent.key, false, false),
                meta(sys, false, false),
            ],
            data,
        };
        ctx.invoke(ix, &pool)?;
    }
    Ok(())
}

/// Farms user state for an obligation: `["user", farm_state, obligation]` of the Farms program.
pub fn farm_user_state_address(farm_state: &Pubkey, obligation: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"user", farm_state.as_ref(), obligation.as_ref()], &FARMS_PROGRAM_ID).0
}

// ------------------------------------------------------------ reads

/// (borrow_apy_bps, supply_apy_bps) of the USDC reserve. Mock builds take the
/// keeper's values; otherwise both are derived from the reserve account.
pub fn read_rates(reserve: &AccountInfo, mock: Option<(u32, u32)>) -> Result<(u32, u32)> {
    match (super::MOCK, mock) {
        (true, Some(v)) => Ok(v),
        (true, None) => err!(CarreraError::InvalidArgument),
        (false, Some(_)) => err!(CarreraError::MockNotAllowed),
        (false, None) => {
            let d = reserve.try_borrow_data()?;
            require!(d.len() == reserve::LEN + 8 && *reserve.owner == KLEND_PROGRAM_ID, CarreraError::VenueAccountsMismatch);
            rates_from_reserve(&d).ok_or_else(|| error!(CarreraError::MathOverflow))
        }
    }
}

/// xStock price, USD × 1e6 per whole unit. Mock builds take the keeper's value;
/// otherwise `oracle` is the xStock reserve, refreshed first through
/// `refresh_reserve` when `[klend_program, lending_market, scope_prices]` are supplied
/// as remaining accounts, and its `market_price_sf` must be at most `MAX_PRICE_AGE_SECS` old.
pub fn read_price<'a, 'info>(ctx: &VenueCtx<'a, 'info>, oracle: &AccountInfo<'info>, mock: Option<u64>) -> Result<u64> {
    match (super::MOCK, mock) {
        (true, Some(p)) if p > 0 => Ok(p),
        (true, _) => err!(CarreraError::InvalidArgument),
        (false, Some(_)) => err!(CarreraError::MockNotAllowed),
        (false, None) => {
            require!(*oracle.owner == KLEND_PROGRAM_ID, CarreraError::VenueAccountsMismatch);
            if ctx.remaining.len() >= 3 {
                let (program, market, scope) = (&ctx.remaining[0], &ctx.remaining[1], &ctx.remaining[2]);
                require_keys_eq!(*program.key, KLEND_PROGRAM_ID, CarreraError::VenueAccountsMismatch);
                let ix = Instruction {
                    program_id: KLEND_PROGRAM_ID,
                    accounts: vec![
                        meta(*oracle.key, true, false),
                        meta(*market.key, false, false),
                        meta(KLEND_PROGRAM_ID, false, false),
                        meta(KLEND_PROGRAM_ID, false, false),
                        meta(KLEND_PROGRAM_ID, false, false),
                        meta(*scope.key, false, false),
                    ],
                    data: D_REFRESH_RESERVE.to_vec(),
                };
                let pool = [oracle.clone(), market.clone(), scope.clone(), program.clone()];
                ctx.invoke(ix, &pool)?;
            }
            let d = oracle.try_borrow_data()?;
            require!(d.len() == reserve::LEN + 8, CarreraError::VenueAccountsMismatch);
            require!(pk(&d[8..], reserve::LIQ_MINT) == ctx.xstock_mint, CarreraError::VenueAccountsMismatch);
            let (price, ts) = price_from_reserve(&d).ok_or(CarreraError::MathOverflow)?;
            let now = Clock::get()?.unix_timestamp;
            require!(price > 0 && now - ts <= MAX_PRICE_AGE_SECS, CarreraError::NavStale);
            Ok(price)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obligation_deposited_reads_ctoken_amount() {
        let reserve = Pubkey::new_unique();
        let mut d = vec![0u8; obligation::LEN + 8];
        // Second deposit slot holds our reserve with 1_343_778 cTokens.
        let o = 8 + obligation::DEPOSITS + obligation::DEPOSIT_STRIDE;
        d[o..o + 32].copy_from_slice(reserve.as_ref());
        d[o + 32..o + 40].copy_from_slice(&1_343_778u64.to_le_bytes());
        assert_eq!(obligation_deposited(&d, &reserve), Some(1_343_778));
        assert_eq!(obligation_deposited(&d, &Pubkey::new_unique()), None);
        assert_eq!(obligation_deposited(&d[..100], &reserve), None);
    }

    fn fixture(name: &str) -> Vec<u8> {
        let raw = match name {
            "usdc" => include_str!("../../../../tests/fixtures/kamino/reserve_usdc.json"),
            "tslax" => include_str!("../../../../tests/fixtures/kamino/reserve_tslax.json"),
            _ => unreachable!(),
        };
        let key = "\"data_base64\":";
        let after = raw.find(key).unwrap() + key.len();
        let start = raw[after..].find('"').unwrap() + after + 1;
        let end = raw[start..].find('"').unwrap() + start;
        base64_decode(&raw[start..end])
    }

    fn base64_decode(s: &str) -> Vec<u8> {
        const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::with_capacity(s.len() * 3 / 4);
        let mut buf = 0u32;
        let mut bits = 0;
        for &c in s.as_bytes() {
            if c == b'=' {
                break;
            }
            let v = T.iter().position(|&t| t == c).unwrap() as u32;
            buf = (buf << 6) | v;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((buf >> bits) as u8);
                buf &= (1 << bits) - 1;
            }
        }
        out
    }

    #[test]
    fn discriminators() {
        use sha2::{Digest, Sha256};
        let check = |name: &str, d: [u8; 8]| {
            let h = Sha256::digest(format!("global:{name}").as_bytes());
            assert_eq!(&h[..8], &d, "{name}");
        };
        check("init_user_metadata", D_INIT_USER_METADATA);
        check("init_obligation", D_INIT_OBLIGATION);
        check("refresh_reserve", D_REFRESH_RESERVE);
        check("refresh_obligation", D_REFRESH_OBLIGATION);
        check("deposit_reserve_liquidity_and_obligation_collateral_v2", D_DEPOSIT_COLL_V2);
        check("withdraw_obligation_collateral_and_redeem_reserve_collateral_v2", D_WITHDRAW_COLL_V2);
        check("borrow_obligation_liquidity_v2", D_BORROW_V2);
        check("repay_obligation_liquidity_v2", D_REPAY_V2);
        check("deposit_reserve_liquidity", D_DEPOSIT_LIQUIDITY);
        check("redeem_reserve_collateral", D_REDEEM_COLLATERAL);
        check("init_obligation_farms_for_reserve", D_INIT_OBLIGATION_FARMS);
    }

    #[test]
    fn reserve_fixture_parses() {
        let usdc = fixture("usdc");
        assert_eq!(usdc.len(), reserve::LEN + 8);
        let d = &usdc[8..];
        assert_eq!(pk(d, reserve::LIQ_MINT), pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"));
        assert_eq!(u64_at(d, reserve::LIQ_DECIMALS), 6);
        assert_eq!(pk(d, reserve::FARM_COLLATERAL), Pubkey::default());
        assert_eq!(pk(d, reserve::FARM_DEBT), pubkey!("82eHAjSXZEyA3UpBxTjVYXF4QJmAEtLR6kvWXQca7mqd"), "USDC reserve has a debt farm");
        let (price, ts) = price_from_reserve(&usdc).unwrap();
        assert!((999_000..=1_001_000).contains(&price), "usdc price {price}");
        assert!(ts > 1_700_000_000);
        let (borrow, supply) = rates_from_reserve(&usdc).unwrap();
        assert!((300..1500).contains(&borrow), "borrow {borrow}");
        assert!(supply < borrow && supply > 0, "supply {supply}");

        let tsla = fixture("tslax");
        let d = &tsla[8..];
        assert_eq!(pk(d, reserve::LIQ_MINT), pubkey!("XsDoVfqeBukxuZHWhdvWHBhgEHjGNst4MLodqsJHzoB"));
        assert_eq!(u64_at(d, reserve::LIQ_DECIMALS), 8);
        assert_eq!(pk(d, reserve::LIQ_TOKEN_PROGRAM), pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"));
        assert_eq!(pk(d, reserve::LENDING_MARKET), pubkey!("5wJeMrUYECGq41fxRESKALVcHnNX26TAWy4W98yULsua"));
        let (price, _) = price_from_reserve(&tsla).unwrap();
        assert!((100_000_000..1_000_000_000).contains(&price), "tsla price {price}");
        // Exchange rate: withdrawing 1 unit needs ≥ 1 cToken-equivalent, never 0.
        assert!(liquidity_to_collateral(&tsla, 1).unwrap() >= 1);
        let big = liquidity_to_collateral(&tsla, 100_000_000).unwrap();
        assert!((90_000_000..=110_000_000).contains(&big), "ctoken per 1 TSLAx: {big}");
    }

    #[test]
    fn obligation_pda_is_deterministic() {
        let v = Pubkey::new_unique();
        let m = pubkey!("5wJeMrUYECGq41fxRESKALVcHnNX26TAWy4W98yULsua");
        assert_eq!(obligation_address(&v, &m), obligation_address(&v, &m));
        assert_ne!(obligation_address(&v, &m), user_metadata_address(&v));
    }
}
