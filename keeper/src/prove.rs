//! `venues prove-jupiter`: build the real-build `wind_step(1)` transaction for a vault with a live
//! Jupiter route, simulate it against the target RPC, and dump every account the route touches
//! (plus the programs it invokes) as fixtures for the LiteSVM fork test
//! (`program/fork-tests/tests/kamino_fork.rs::jupiter_wind_step_one_in_fork`).
//!
//! Nothing is sent. Against mainnet the simulation is expected to fail inside `carrera_overlay`
//! because the deployed program is still the mock-venues build; the fork test is where the swap
//! is proven to execute through the program's own out-mint / min-out checks.

use crate::{
    config::{Config, VaultCfg},
    venue::{self, PhoenixKeys, Swap},
    venue_accounts::{jupiter_route, KaminoBlock, ReserveInfo, VenueArgs, VenueData, BLOCK_JUPITER, BLOCK_KAMINO, BLOCK_PHOENIX},
    venue_setup::jupiter_base_url,
    Ctx,
};
use anyhow::{anyhow, Context as _, Result};
use serde_json::json;
use solana_sdk::{instruction::AccountMeta, pubkey::Pubkey};
use std::path::Path;

const BPF_LOADER_UPGRADEABLE: Pubkey = solana_sdk::pubkey!("BPFLoaderUpgradeab1e11111111111111111111111");

fn b64(bytes: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Trim an ELF to its real extent (programdata is padded to the allocated size).
fn trim_elf(bytes: &[u8]) -> &[u8] {
    if bytes.len() < 0x40 || &bytes[..4] != b"\x7fELF" {
        return bytes;
    }
    let shoff = u64::from_le_bytes(bytes[0x28..0x30].try_into().unwrap()) as usize;
    let shentsize = u16::from_le_bytes(bytes[0x3a..0x3c].try_into().unwrap()) as usize;
    let shnum = u16::from_le_bytes(bytes[0x3c..0x3e].try_into().unwrap()) as usize;
    &bytes[..(shoff + shentsize * shnum).min(bytes.len())]
}

pub async fn prove_jupiter(ctx: &Ctx, symbol: &str, amount: u64, out_dir: &Path) -> Result<()> {
    let cfg: &Config = &ctx.cfg;
    let vc: &VaultCfg = cfg
        .vaults
        .iter()
        .find(|v| v.symbol.eq_ignore_ascii_case(symbol))
        .ok_or_else(|| anyhow!("no vault {symbol} in config"))?;
    let chain = &ctx.chain;
    let vault = chain.pdas().vault(&vc.mint);
    let custody = chain.pdas().stock_custody(&vault);
    let buffer = chain.pdas().usdc_buffer(&vault);
    let base = jupiter_base_url(&cfg.jupiter_price_url);

    // 1. Live routes both ways (the reverse is informational).
    let fwd = jupiter_route(&ctx.http, &base, &cfg.usdc_mint, &vc.mint, amount, 50, &vault, &buffer, &custody).await.context("USDC→stock route")?;
    println!("USDC→{}x: {amount} in, quoted out {}, {} accounts, {} bytes, tables {:?}", vc.symbol, fwd.quoted_out_amount, fwd.block.len(), fwd.data.len(), fwd.lookup_tables);
    let rev_amount = fwd.quoted_out_amount.max(1);
    match jupiter_route(&ctx.http, &base, &vc.mint, &cfg.usdc_mint, rev_amount, 50, &vault, &custody, &buffer).await {
        Ok(rev) => println!("{}x→USDC: {rev_amount} in, quoted out {}, {} accounts, tables {:?}", vc.symbol, rev.quoted_out_amount, rev.block.len(), rev.lookup_tables),
        Err(e) => println!("{}x→USDC route failed: {e:#}", vc.symbol),
    }

    // 2. The full wind_step(1) instruction as the real build expects it.
    let stock_reserve = vc.kamino_reserve.ok_or_else(|| anyhow!("{}: kamino_reserve not configured", vc.symbol))?;
    if cfg.kamino_reserve == Pubkey::default() {
        return Err(anyhow!("kamino_reserve (USDC) not configured"));
    }
    let accs = chain.rpc.get_multiple_accounts(&[stock_reserve, cfg.kamino_reserve]).await.context("reserves")?;
    let stock = ReserveInfo::parse(&accs[0].as_ref().ok_or_else(|| anyhow!("stock reserve missing"))?.data)?;
    let usdc = ReserveInfo::parse(&accs[1].as_ref().ok_or_else(|| anyhow!("usdc reserve missing"))?.data)?;
    let kamino = KaminoBlock::metas(&vault, &vc.mint, &custody, &buffer, &stock_reserve, &stock, &cfg.kamino_reserve, &usdc)?;
    let keys: PhoenixKeys = venue::fetch_phoenix_keys(&ctx.http, &cfg.phoenix_api_url).await?;
    let market = keys.market(&vc.phoenix_market).ok_or_else(|| anyhow!("Phoenix market {} not listed", vc.phoenix_market))?.clone();
    let phoenix = venue::phoenix_block(&vault, &cfg.usdc_mint, &buffer, &keys, &market);
    let slot = chain.slot().await?;
    let data = VenueData {
        blocks: BLOCK_KAMINO | BLOCK_PHOENIX | BLOCK_JUPITER,
        phoenix_gti: keys.global_trader_index.len() as u8,
        phoenix_atb: keys.active_trader_buffer.len() as u8,
        base_lot_size: venue::base_lot_size(vc.stock_decimals, market.base_lots_decimals),
        last_valid_slot: slot + venue::ORDER_VALID_SLOTS,
        jupiter_data: fwd.data.clone(),
        ..Default::default()
    };
    let build = |blocks: u8, with_phoenix: bool| {
        let mut remaining: Vec<AccountMeta> = Vec::new();
        remaining.extend(kamino.iter().cloned());
        if with_phoenix {
            remaining.extend(phoenix.iter().cloned());
        }
        remaining.extend(fwd.block.iter().cloned());
        let args = VenueArgs::from_data(&VenueData { blocks, ..data.clone() }, remaining, fwd.lookup_tables.clone());
        chain.ix.wind_step(&vault, 1, &args)
    };
    let ix = build(BLOCK_KAMINO | BLOCK_PHOENIX | BLOCK_JUPITER, true);
    println!("wind_step(1): {} accounts ({} kamino + {} phoenix + {} jupiter), venue_data {} bytes, swap {:?}", ix.accounts.len(), kamino.len(), phoenix.len(), fwd.block.len(), fwd.data.len() + 43, Swap::UsdcToStock(amount));

    // 3. Simulate on the target RPC. Tables: the route's, plus Kamino's public table for the
    //    market (covers the Kamino block). The keeper's own per-vault table does not exist until
    //    Phase B, so if the full three-block shape does not fit in a packet, the shape step 1
    //    actually consumes (Kamino + Jupiter) is simulated instead and reported as such.
    let mut tables = fwd.lookup_tables.clone();
    match kamino_market_table(&ctx.http, &cfg.kamino_api_url, &cfg.kamino_market).await {
        Ok(t) => {
            println!("Kamino market lookup table: {t}");
            tables.push(t);
        }
        Err(e) => println!("Kamino market lookup table unavailable: {e:#}"),
    }
    let attempts = [(ix.clone(), "kamino+phoenix+jupiter"), (build(BLOCK_KAMINO | BLOCK_JUPITER, false), "kamino+jupiter (what step 1 consumes)")];
    for (ix, shape) in attempts {
        match chain.simulate_ixs(vec![ix], &tables).await {
            Ok(logs) => {
                println!("simulation [{shape}] succeeded ({} log lines)", logs.len());
                for l in logs.iter().rev().take(6).rev() {
                    println!("  {l}");
                }
                break;
            }
            Err(e) => {
                let text = format!("{e:#}");
                let too_large = text.contains("too large");
                println!("simulation [{shape}] failed{}:", if too_large { " to fit in a packet without the keeper's per-vault lookup table" } else { " on this RPC (expected while the deployed program is the mock build)" });
                let lines: Vec<&str> = text.lines().collect();
                if let Some(first) = lines.first() {
                    println!("  {first}");
                }
                for l in lines.iter().skip(1).rev().take(8).rev() {
                    println!("  {l}");
                }
                if !too_large {
                    break;
                }
            }
        }
    }

    // 4. Dump fixtures: every route account, plus the programs they belong to. Stale files from an
    //    earlier route are removed first so the directory describes exactly this scenario.
    std::fs::create_dir_all(out_dir)?;
    for entry in std::fs::read_dir(out_dir)? {
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with("acct_") || name.starts_with("program_") {
            std::fs::remove_file(&path)?;
        }
    }
    let keys_to_dump: Vec<Pubkey> = fwd.block.iter().map(|m| m.pubkey).collect();
    let mut dumped = 0usize;
    let mut programs: Vec<Pubkey> = Vec::new();
    let mut missing: Vec<Pubkey> = Vec::new();
    for chunk in keys_to_dump.chunks(50) {
        let accs = chain.rpc.get_multiple_accounts(chunk).await.context("route accounts")?;
        for (k, acc) in chunk.iter().zip(accs) {
            match acc {
                None => missing.push(*k),
                Some(a) => {
                    if a.executable {
                        if !programs.contains(k) && *k != cfg.program_id {
                            programs.push(*k);
                        }
                        continue;
                    }
                    if !programs.contains(&a.owner) && a.owner != solana_sdk::system_program::ID && !is_native_program(&a.owner) && a.owner != cfg.program_id {
                        programs.push(a.owner);
                    }
                    let f = json!({ "pubkey": k.to_string(), "owner": a.owner.to_string(), "lamports": a.lamports, "executable": false, "data_base64": b64(&a.data) });
                    std::fs::write(out_dir.join(format!("acct_{k}.json")), serde_json::to_string(&f)?)?;
                    dumped += 1;
                }
            }
        }
    }
    let mut program_files = Vec::new();
    for p in &programs {
        if is_native_program(p) {
            continue;
        }
        let acc = match chain.rpc.get_account(p).await {
            Ok(a) if a.owner == BPF_LOADER_UPGRADEABLE || a.owner.to_string().starts_with("BPFLoader") => a,
            _ => continue,
        };
        let elf: Vec<u8> = if acc.owner == BPF_LOADER_UPGRADEABLE && acc.data.len() >= 36 && acc.data[..4] == [2, 0, 0, 0] {
            let pd = Pubkey::new_from_array(acc.data[4..36].try_into().unwrap());
            let pda = chain.rpc.get_account(&pd).await.with_context(|| format!("programdata for {p}"))?;
            trim_elf(&pda.data[45..]).to_vec()
        } else {
            trim_elf(&acc.data).to_vec()
        };
        let f = json!({ "pubkey": p.to_string(), "owner": acc.owner.to_string(), "lamports": acc.lamports, "executable": true, "elf": true, "data_base64": b64(&elf) });
        let name = format!("program_{p}.json");
        std::fs::write(out_dir.join(&name), serde_json::to_string(&f)?)?;
        program_files.push(name);
    }
    let block_json: Vec<serde_json::Value> = fwd.block.iter().map(|m| json!({ "pubkey": m.pubkey.to_string(), "is_writable": m.is_writable, "is_signer": m.is_signer })).collect();
    let scenario = json!({
        "vault_symbol": vc.symbol,
        "vault": vault.to_string(),
        "input_mint": cfg.usdc_mint.to_string(),
        "output_mint": vc.mint.to_string(),
        "amount": amount,
        "quoted_out_amount": fwd.quoted_out_amount,
        "data_base64": b64(&fwd.data),
        "block": block_json,
        "lookup_tables": fwd.lookup_tables.iter().map(|k| k.to_string()).collect::<Vec<_>>(),
        "programs": program_files,
        "missing_accounts": missing.iter().map(|k| k.to_string()).collect::<Vec<_>>(),
        "dump_slot": slot,
        "dump_ts": chrono::Utc::now().timestamp(),
    });
    std::fs::write(out_dir.join("scenario.json"), serde_json::to_string_pretty(&scenario)?)?;
    println!("fixtures: {dumped} accounts, {} programs, {} missing (PDAs that do not exist yet) → {}", programs.len(), missing.len(), out_dir.display());
    Ok(())
}

/// Kamino publishes one address lookup table per market (`/v2/kamino-market/<market>`).
async fn kamino_market_table(http: &reqwest::Client, api: &str, market: &str) -> Result<Pubkey> {
    let v: serde_json::Value = http.get(format!("{api}/v2/kamino-market/{market}")).send().await?.error_for_status()?.json().await?;
    v["lookupTable"].as_str().ok_or_else(|| anyhow!("no lookupTable in market metadata"))?.parse().context("lookupTable")
}

fn is_native_program(p: &Pubkey) -> bool {
    const NATIVE: [Pubkey; 6] = [
        solana_sdk::pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"),
        solana_sdk::pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"),
        solana_sdk::pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"),
        solana_sdk::pubkey!("11111111111111111111111111111111"),
        solana_sdk::pubkey!("ComputeBudget111111111111111111111111111111"),
        solana_sdk::pubkey!("BPFLoaderUpgradeab1e11111111111111111111111"),
    ];
    NATIVE.contains(p) || p.to_string().starts_with("Sysvar")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trip_shapes() {
        assert_eq!(b64(b""), "");
        assert_eq!(b64(b"f"), "Zg==");
        assert_eq!(b64(b"fo"), "Zm8=");
        assert_eq!(b64(b"foo"), "Zm9v");
        assert_eq!(b64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn elf_trim_leaves_non_elf_alone() {
        assert_eq!(trim_elf(b"hello"), b"hello");
    }
}
