//! Venue accounts and parameters for the program's real Kamino / Jupiter / Phoenix
//! legs (program `venues` module, docs/CONTRACT.md "Venue blocks").
//!
//! Every engine crank takes a trailing Borsh `VenueData` argument and, when it
//! executes a venue leg, the corresponding account blocks in `remaining_accounts`
//! (Kamino 22, Phoenix 16 + trader-index accounts, Jupiter = the swap
//! instruction's accounts). Against the mock-venues build the keeper sends
//! `VenueArgs::none()`, which is four zero bytes and no extra accounts.

use anyhow::{anyhow, Context, Result};
use borsh::{BorshDeserialize, BorshSerialize};
use serde::Deserialize;
use solana_sdk::instruction::AccountMeta;
use solana_sdk::pubkey::Pubkey;

pub const KLEND_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("KLend2g3cP87fffoy8q1mQqGKjrxjC8boSyAYavgmjD");
pub const FARMS_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("FarmsPZpWu9i7Kky8tPN37rs2TpmMrAZrC7S7vJa91Hr");
pub const JUPITER_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4");
pub const TOKEN_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
pub const ASSOCIATED_TOKEN_PROGRAM_ID: Pubkey = solana_sdk::pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

pub const BLOCK_KAMINO: u8 = 1;
/// Assembled by the loops once `venues = "onchain"` lands (Phoenix block: trader-index accounts from the Phoenix SDK).
#[allow(dead_code)]
pub const BLOCK_PHOENIX: u8 = 2;
#[allow(dead_code)]
pub const BLOCK_JUPITER: u8 = 4;

/// Mirror of the program's `venues::VenueData` (Borsh, same field order).
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct VenueData {
    pub blocks: u8,
    pub phoenix_gti: u8,
    pub phoenix_atb: u8,
    pub base_lot_size: u64,
    pub price_in_ticks: u64,
    pub last_valid_slot: u64,
    pub phoenix_equity_usdc: u64,
    pub client_order_id: u64,
    pub jupiter_data: Vec<u8>,
}

/// What an engine crank needs beyond its fixed accounts.
#[derive(Clone, Debug, Default)]
pub struct VenueArgs {
    pub data: Vec<u8>,
    pub remaining: Vec<AccountMeta>,
    /// Address lookup tables the transaction must load (Jupiter routes).
    pub lookup_tables: Vec<Pubkey>,
}

impl VenueArgs {
    /// Empty venue data: what the mock-venues build expects.
    pub fn none() -> Self {
        Self::default()
    }

    pub fn from_data(data: &VenueData, remaining: Vec<AccountMeta>, lookup_tables: Vec<Pubkey>) -> Self {
        Self { data: borsh::to_vec(data).expect("borsh"), remaining, lookup_tables }
    }
}

// ------------------------------------------------------------ Kamino

/// Fields the keeper needs from a klend `Reserve` account (offsets after the 8-byte
/// discriminator; same table as the program's `venues::kamino::reserve`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReserveInfo {
    pub lending_market: Pubkey,
    pub liquidity_mint: Pubkey,
    pub liquidity_supply: Pubkey,
    pub fee_vault: Pubkey,
    pub token_program: Pubkey,
    pub collateral_mint: Pubkey,
    pub collateral_supply: Pubkey,
    pub scope_price_feed: Pubkey,
    pub decimals: u8,
    /// Debt farm (Farms program state), `None` when the reserve has none.
    pub farm_debt: Option<Pubkey>,
}

impl ReserveInfo {
    pub const LEN: usize = 8616 + 8;

    pub fn parse(account_data: &[u8]) -> Result<Self> {
        if account_data.len() != Self::LEN {
            return Err(anyhow!("reserve account is {} bytes, expected {}", account_data.len(), Self::LEN));
        }
        let d = &account_data[8..];
        let pk = |o: usize| Pubkey::new_from_array(d[o..o + 32].try_into().unwrap());
        Ok(Self {
            lending_market: pk(24),
            liquidity_mint: pk(120),
            liquidity_supply: pk(152),
            fee_vault: pk(184),
            token_program: pk(400),
            collateral_mint: pk(2552),
            collateral_supply: pk(2592),
            scope_price_feed: pk(5104),
            decimals: u64::from_le_bytes(d[264..272].try_into().unwrap()) as u8,
            farm_debt: Some(pk(88)).filter(|k| *k != Pubkey::default()),
        })
    }
}

pub fn lending_market_authority(market: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"lma", market.as_ref()], &KLEND_PROGRAM_ID).0
}

/// The vault's obligation: tag 0, id 0, seed accounts = system program.
pub fn obligation_address(vault: &Pubkey, market: &Pubkey) -> Pubkey {
    let sys = solana_sdk::system_program::ID;
    Pubkey::find_program_address(
        &[&[0u8], &[0u8], vault.as_ref(), market.as_ref(), sys.as_ref(), sys.as_ref()],
        &KLEND_PROGRAM_ID,
    )
    .0
}

/// Farms user state for an obligation: `["user", farm_state, obligation]`.
pub fn farm_user_state_address(farm_state: &Pubkey, obligation: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"user", farm_state.as_ref(), obligation.as_ref()], &FARMS_PROGRAM_ID).0
}

pub fn user_metadata_address(vault: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"user_meta", vault.as_ref()], &KLEND_PROGRAM_ID).0
}

pub fn associated_token_address(owner: &Pubkey, mint: &Pubkey, token_program: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[owner.as_ref(), token_program.as_ref(), mint.as_ref()], &ASSOCIATED_TOKEN_PROGRAM_ID).0
}

/// The 24-account Kamino block for one vault.
pub struct KaminoBlock;

impl KaminoBlock {
    #[allow(clippy::too_many_arguments)]
    pub fn metas(
        vault: &Pubkey,
        xstock_mint: &Pubkey,
        stock_custody: &Pubkey,
        usdc_buffer: &Pubkey,
        stock_reserve: &Pubkey,
        stock: &ReserveInfo,
        usdc_reserve: &Pubkey,
        usdc: &ReserveInfo,
    ) -> Result<Vec<AccountMeta>> {
        if stock.lending_market != usdc.lending_market {
            return Err(anyhow!("reserves belong to different markets"));
        }
        if &stock.liquidity_mint != xstock_mint {
            return Err(anyhow!("stock reserve mint {} is not the vault mint {}", stock.liquidity_mint, xstock_mint));
        }
        let market = stock.lending_market;
        let obligation = obligation_address(vault, &market);
        let w = |k: Pubkey| AccountMeta::new(k, false);
        let r = |k: Pubkey| AccountMeta::new_readonly(k, false);
        let (farm, farm_user) = match usdc.farm_debt {
            Some(f) => (w(f), w(farm_user_state_address(&f, &obligation))),
            None => (r(KLEND_PROGRAM_ID), r(KLEND_PROGRAM_ID)),
        };
        Ok(vec![
            r(KLEND_PROGRAM_ID),
            r(market),
            r(lending_market_authority(&market)),
            w(obligation),
            w(*stock_reserve),
            w(stock.liquidity_supply),
            w(stock.collateral_mint),
            w(stock.collateral_supply),
            r(*xstock_mint),
            w(*usdc_reserve),
            w(usdc.liquidity_supply),
            w(usdc.fee_vault),
            w(usdc.collateral_mint),
            w(associated_token_address(vault, &usdc.collateral_mint, &TOKEN_PROGRAM_ID)),
            r(usdc.liquidity_mint),
            r(stock.scope_price_feed),
            r(FARMS_PROGRAM_ID),
            r(solana_sdk::sysvar::instructions::ID),
            r(TOKEN_PROGRAM_ID),
            r(stock.token_program),
            w(*stock_custody),
            w(*usdc_buffer),
            farm,
            farm_user,
        ])
    }
}

// ------------------------------------------------------------ Jupiter

#[derive(Deserialize)]
struct SwapIxJson {
    #[serde(rename = "programId")]
    program_id: String,
    accounts: Vec<SwapAccountJson>,
    data: String,
}

#[derive(Deserialize)]
struct SwapAccountJson {
    pubkey: String,
    #[serde(rename = "isSigner")]
    is_signer: bool,
    #[serde(rename = "isWritable")]
    is_writable: bool,
}

#[derive(Deserialize)]
struct SwapInstructionsJson {
    #[serde(rename = "swapInstruction")]
    swap_instruction: SwapIxJson,
    #[serde(rename = "addressLookupTableAddresses", default)]
    lookup_tables: Vec<String>,
}

/// A Jupiter `shared_accounts_route` ready for the program's Jupiter block.
#[derive(Clone, Debug)]
pub struct JupiterRoute {
    /// Instruction data as returned by the API (the program patches the amounts).
    pub data: Vec<u8>,
    /// `[jupiter_program, swap accounts...]` with the vault's PDA token accounts
    /// substituted for the user source/destination and the vault as authority.
    pub block: Vec<AccountMeta>,
    pub lookup_tables: Vec<Pubkey>,
    pub quoted_out_amount: u64,
}

/// Map a swap-instructions API response into the Jupiter block. `user_source` and
/// `user_dest` replace the ATAs Jupiter derived for `vault` (indices 3 and 6).
pub fn jupiter_block_from_response(
    json: &str,
    vault: &Pubkey,
    user_source: &Pubkey,
    user_dest: &Pubkey,
    quoted_out_amount: u64,
) -> Result<JupiterRoute> {
    let resp: SwapInstructionsJson = serde_json::from_str(json).context("swap-instructions json")?;
    let ix = resp.swap_instruction;
    if ix.program_id.parse::<Pubkey>()? != JUPITER_PROGRAM_ID {
        return Err(anyhow!("swap instruction is not for Jupiter v6"));
    }
    let data = base64_decode(&ix.data)?;
    if data.len() < 8 || data[..8] != [193, 32, 155, 51, 65, 214, 156, 129] {
        return Err(anyhow!("swap instruction is not shared_accounts_route"));
    }
    let mut block = vec![AccountMeta::new_readonly(JUPITER_PROGRAM_ID, false)];
    for (i, a) in ix.accounts.iter().enumerate() {
        let mut key: Pubkey = a.pubkey.parse()?;
        match i {
            2 => {
                if &key != vault {
                    return Err(anyhow!("route authority {key} is not the vault {vault}"));
                }
            }
            3 => key = *user_source,
            6 => key = *user_dest,
            _ => {}
        }
        block.push(AccountMeta { pubkey: key, is_signer: a.is_signer && i == 2, is_writable: a.is_writable });
    }
    let lookup_tables = resp.lookup_tables.iter().map(|s| s.parse()).collect::<std::result::Result<Vec<Pubkey>, _>>()?;
    Ok(JupiterRoute { data, block, lookup_tables, quoted_out_amount })
}

/// Quote and build a route for `amount` of `input_mint` → `output_mint` signed by the vault.
#[allow(clippy::too_many_arguments)]
pub async fn jupiter_route(
    client: &reqwest::Client,
    base_url: &str,
    input_mint: &Pubkey,
    output_mint: &Pubkey,
    amount: u64,
    slippage_bps: u16,
    vault: &Pubkey,
    user_source: &Pubkey,
    user_dest: &Pubkey,
) -> Result<JupiterRoute> {
    let quote_url = format!(
        "{base_url}/swap/v1/quote?inputMint={input_mint}&outputMint={output_mint}&amount={amount}&slippageBps={slippage_bps}&maxAccounts=40"
    );
    let quote: serde_json::Value = client.get(&quote_url).send().await?.error_for_status()?.json().await?;
    let out: u64 = quote["outAmount"].as_str().unwrap_or("0").parse().unwrap_or(0);
    let body = serde_json::json!({
        "quoteResponse": quote,
        "userPublicKey": vault.to_string(),
        "wrapAndUnwrapSol": false,
        "useSharedAccounts": true,
        "dynamicComputeUnitLimit": false,
        "skipUserAccountsRpcCalls": true,
    });
    let text = client
        .post(format!("{base_url}/swap/v1/swap-instructions"))
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    jupiter_block_from_response(&text, vault, user_source, user_dest, out)
}

fn base64_decode(s: &str) -> Result<Vec<u8>> {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut buf, mut bits) = (0u32, 0);
    for &c in s.as_bytes() {
        if c == b'=' {
            break;
        }
        let v = T.iter().position(|&t| t == c).ok_or_else(|| anyhow!("bad base64"))? as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_bytes(json: &str) -> Vec<u8> {
        let key = "\"data_base64\":";
        let after = json.find(key).unwrap() + key.len();
        let start = json[after..].find('"').unwrap() + after + 1;
        let end = json[start..].find('"').unwrap() + start;
        base64_decode(&json[start..end]).unwrap()
    }

    #[test]
    fn venue_data_none_is_four_zero_bytes_of_length_and_default_fields() {
        let none = VenueArgs::none();
        assert!(none.data.is_empty() && none.remaining.is_empty());
        let d = VenueData { blocks: BLOCK_KAMINO | BLOCK_JUPITER, jupiter_data: vec![1, 2, 3], ..Default::default() };
        let bytes = borsh::to_vec(&d).unwrap();
        assert_eq!(bytes.len(), 3 + 8 * 5 + 4 + 3);
        assert_eq!(VenueData::try_from_slice(&bytes).unwrap(), d);
    }

    #[test]
    fn kamino_block_from_live_reserves() {
        let usdc = ReserveInfo::parse(&fixture_bytes(include_str!("../../program/tests/fixtures/kamino/reserve_usdc.json"))).unwrap();
        let tsla = ReserveInfo::parse(&fixture_bytes(include_str!("../../program/tests/fixtures/kamino/reserve_tslax.json"))).unwrap();
        assert_eq!(usdc.liquidity_mint, solana_sdk::pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"));
        assert_eq!(tsla.liquidity_mint, solana_sdk::pubkey!("XsDoVfqeBukxuZHWhdvWHBhgEHjGNst4MLodqsJHzoB"));
        assert_eq!(tsla.token_program, solana_sdk::pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"));
        assert_eq!(tsla.decimals, 8);
        let vault = solana_sdk::pubkey!("14pBdW5byHDDAXSnhCoortKakZqKYEWwd4i8F4VzR3EM");
        let m = KaminoBlock::metas(
            &vault,
            &tsla.liquidity_mint,
            &Pubkey::new_unique(),
            &Pubkey::new_unique(),
            &solana_sdk::pubkey!("5iTiczqgUegqA3PpoNpotizMbY9n1sRWr3oL6igKvWuf"),
            &tsla,
            &solana_sdk::pubkey!("97zoywd8mPZsGTg8q1wdD2Wgkdrs2tqusp1Qqcxbyj7E"),
            &usdc,
        )
        .unwrap();
        assert_eq!(m.len(), 24);
        assert_eq!(m[22].pubkey, usdc.farm_debt.unwrap(), "USDC debt farm");
        assert!(m[23].is_writable);
        assert_eq!(m[1].pubkey, solana_sdk::pubkey!("5wJeMrUYECGq41fxRESKALVcHnNX26TAWy4W98yULsua"));
        assert_eq!(m[15].pubkey, solana_sdk::pubkey!("3t4JZcueEzTbVP6kLxXrL3VpWx45jDer4eqysweBchNH"));
        assert_eq!(m[3].pubkey, obligation_address(&vault, &usdc.lending_market));
        assert!(m[3].is_writable && !m[0].is_writable);
        // Mismatched vault mint is refused.
        assert!(KaminoBlock::metas(&vault, &Pubkey::new_unique(), &vault, &vault, &vault, &tsla, &vault, &usdc).is_err());
    }

    #[test]
    fn jupiter_block_from_captured_swap_instructions() {
        let json = include_str!("../tests/fixtures/jupiter_swap_instructions.json");
        let vault = solana_sdk::pubkey!("14pBdW5byHDDAXSnhCoortKakZqKYEWwd4i8F4VzR3EM");
        let (src, dst) = (Pubkey::new_unique(), Pubkey::new_unique());
        let r = jupiter_block_from_response(json, &vault, &src, &dst, 793_680).unwrap();
        assert_eq!(r.block[0].pubkey, JUPITER_PROGRAM_ID);
        assert_eq!(r.block.len(), 1 + 29);
        assert_eq!(r.block[3].pubkey, vault);
        assert!(r.block[3].is_signer);
        assert_eq!(r.block[4].pubkey, src);
        assert_eq!(r.block[7].pubkey, dst);
        assert_eq!(&r.data[..8], &[193, 32, 155, 51, 65, 214, 156, 129]);
        assert_eq!(r.lookup_tables.len(), 1);
        assert!(jupiter_block_from_response(json, &Pubkey::new_unique(), &src, &dst, 0).is_err(), "wrong authority");
    }
}
