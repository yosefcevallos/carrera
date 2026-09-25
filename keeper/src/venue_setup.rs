//! Operator commands for the real venue legs: build a vault's Kamino block from
//! chain, create its obligation, and dry-run a Jupiter route. These are the
//! building blocks the loops use once `venues = "onchain"` is switched on.

use crate::chain::Chain;
use crate::config::{Config, VaultCfg};
use crate::venue_accounts::{
    associated_token_address, jupiter_route, user_metadata_address, KaminoBlock, ReserveInfo, VenueArgs, VenueData,
    ASSOCIATED_TOKEN_PROGRAM_ID, BLOCK_KAMINO, TOKEN_PROGRAM_ID,
};
use anyhow::{anyhow, Context, Result};
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;

pub fn vault_cfg<'a>(cfg: &'a Config, symbol: &str) -> Result<&'a VaultCfg> {
    cfg.vaults
        .iter()
        .find(|v| v.symbol.eq_ignore_ascii_case(symbol))
        .ok_or_else(|| anyhow!("no vault {symbol} in config"))
}

/// Fetch both reserves and assemble the 22-account Kamino block for a vault.
pub async fn kamino_block(chain: &Chain, cfg: &Config, vc: &VaultCfg) -> Result<(VenueArgs, Pubkey)> {
    let stock_reserve = vc.kamino_reserve.ok_or_else(|| anyhow!("{}: kamino_reserve not configured", vc.symbol))?;
    let usdc_reserve = cfg.kamino_reserve;
    let accs = chain.rpc.get_multiple_accounts(&[stock_reserve, usdc_reserve]).await.context("reserves")?;
    let stock = ReserveInfo::parse(&accs[0].as_ref().ok_or_else(|| anyhow!("stock reserve missing"))?.data)?;
    let usdc = ReserveInfo::parse(&accs[1].as_ref().ok_or_else(|| anyhow!("usdc reserve missing"))?.data)?;
    let vault = chain.pdas().vault(&vc.mint);
    let metas = KaminoBlock::metas(
        &vault,
        &vc.mint,
        &chain.pdas().stock_custody(&vault),
        &chain.pdas().usdc_buffer(&vault),
        &stock_reserve,
        &stock,
        &usdc_reserve,
        &usdc,
    )?;
    let data = VenueData { blocks: BLOCK_KAMINO, ..Default::default() };
    Ok((VenueArgs::from_data(&data, metas, vec![]), vault))
}

pub async fn print_kamino_block(chain: &Chain, cfg: &Config, symbol: &str) -> Result<()> {
    let vc = vault_cfg(cfg, symbol)?;
    let (args, vault) = kamino_block(chain, cfg, vc).await?;
    println!("vault {vault}");
    println!("venue_data (hex) {}", hex(&args.data));
    for (i, m) in args.remaining.iter().enumerate() {
        println!("{i:2} {} {}", m.pubkey, if m.is_writable { "w" } else { "-" });
    }
    Ok(())
}

/// Idempotent create of the vault's associated token account for `mint` under `token_program`
/// (the program validates these accounts exist and belong to the vault before any Kamino CPI).
fn create_vault_ata_ix(payer: &Pubkey, vault: &Pubkey, mint: &Pubkey, token_program: &Pubkey) -> Instruction {
    let ata = associated_token_address(vault, mint, token_program);
    Instruction {
        program_id: ASSOCIATED_TOKEN_PROGRAM_ID,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new(ata, false),
            AccountMeta::new_readonly(*vault, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(solana_sdk::system_program::id(), false),
            AccountMeta::new_readonly(*token_program, false),
        ],
        data: vec![1], // CreateIdempotent
    }
}

/// Send `init_kamino_obligation` for a vault (idempotent on the program side), creating the
/// vault's USDC cToken account first (the program checks it exists and is owned by the vault).
pub async fn init_obligation(chain: &Chain, cfg: &Config, symbol: &str) -> Result<()> {
    let vc = vault_cfg(cfg, symbol)?;
    let usdc_acc = chain.rpc.get_account(&cfg.kamino_reserve).await.context("usdc reserve")?;
    let usdc = ReserveInfo::parse(&usdc_acc.data)?;
    let vault = chain.pdas().vault(&vc.mint);
    let ata_ix = create_vault_ata_ix(&chain.keeper(), &vault, &usdc.collateral_mint, &TOKEN_PROGRAM_ID);
    chain.send(&format!("{symbol} create vault usdc ctoken ata"), ata_ix).await?;
    let (mut args, vault) = kamino_block(chain, cfg, vc).await?;
    args.remaining.push(AccountMeta::new(user_metadata_address(&vault), false));
    let ix = chain.ix.init_kamino_obligation(&vault, &args);
    chain.send(&format!("{symbol} init_kamino_obligation"), ix).await?;
    Ok(())
}

/// Dry-run a Jupiter route for the vault: USDC → stock when `to_stock`, else stock → USDC.
pub async fn print_jupiter_route(chain: &Chain, cfg: &Config, symbol: &str, amount: u64, to_stock: bool) -> Result<()> {
    let vc = vault_cfg(cfg, symbol)?;
    let vault = chain.pdas().vault(&vc.mint);
    let (custody, buffer) = (chain.pdas().stock_custody(&vault), chain.pdas().usdc_buffer(&vault));
    let (input, output, src, dst) = if to_stock {
        (cfg.usdc_mint, vc.mint, buffer, custody)
    } else {
        (vc.mint, cfg.usdc_mint, custody, buffer)
    };
    let base = jupiter_base_url(&cfg.jupiter_price_url);
    let client = reqwest::Client::new();
    let route = jupiter_route(&client, &base, &input, &output, amount, 50, &vault, &src, &dst).await?;
    println!("quoted out {}  data {} bytes  accounts {}  lookup tables {:?}", route.quoted_out_amount, route.data.len(), route.block.len(), route.lookup_tables);
    for (i, m) in route.block.iter().enumerate() {
        println!("{i:2} {} {}{}", m.pubkey, if m.is_writable { "w" } else { "-" }, if m.is_signer { "s" } else { "" });
    }
    Ok(())
}

/// `https://lite-api.jup.ag/price/v3` → `https://lite-api.jup.ag`.
pub fn jupiter_base_url(price_url: &str) -> String {
    match price_url.find("/price") {
        Some(i) => price_url[..i].to_string(),
        None => price_url.trim_end_matches('/').to_string(),
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_strips_price_path() {
        assert_eq!(jupiter_base_url("https://lite-api.jup.ag/price/v3"), "https://lite-api.jup.ag");
        assert_eq!(jupiter_base_url("https://lite-api.jup.ag/"), "https://lite-api.jup.ag");
    }
}
