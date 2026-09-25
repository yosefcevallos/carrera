//! Venue proofs (`venues prove-jupiter`, `venues prove-phoenix`). Nothing is sent.
//!
//! Each command builds the real-build transactions the loops would send for a vault, simulates
//! them against the configured RPC, and dumps every mainnet account (and program) the venue
//! touches as fixtures for the LiteSVM fork tests in `program/fork-tests/tests/kamino_fork.rs`.
//! Against mainnet the crank simulations are expected to stop at the deployed program's state
//! check while it is still the mock-venues build; the fork tests are where the legs are proven to
//! execute end to end.

use crate::{
    config::{Config, VaultCfg},
    venue::{self, PhoenixKeys, Swap},
    venue_accounts::{
        associated_token_address, jupiter_route_with, JupiterRoute, KaminoBlock, ReserveInfo, VenueArgs, VenueData, BLOCK_JUPITER, BLOCK_KAMINO,
        BLOCK_PHOENIX, TOKEN_PROGRAM_ID,
    },
    venue_setup::jupiter_base_url,
    Ctx,
};
use anyhow::{anyhow, Context as _, Result};
use serde_json::json;
use solana_client::rpc_config::RpcAccountInfoConfig;
use solana_sdk::{commitment_config::CommitmentConfig, instruction::AccountMeta, instruction::Instruction, pubkey::Pubkey};
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

fn metas_json(metas: &[AccountMeta]) -> Vec<serde_json::Value> {
    metas.iter().map(|m| json!({ "pubkey": m.pubkey.to_string(), "is_writable": m.is_writable, "is_signer": m.is_signer })).collect()
}

fn ix_json(ix: &Instruction) -> serde_json::Value {
    json!({ "program_id": ix.program_id.to_string(), "accounts": metas_json(&ix.accounts), "data_base64": b64(&ix.data) })
}

/// What a fixture dump produced.
struct Dump {
    accounts: usize,
    programs: Vec<String>,
    missing: Vec<Pubkey>,
    /// Slot the last account batch was read at.
    slot: u64,
}

/// Clear stale `acct_*` / `program_*` files so the directory describes exactly this scenario.
fn clear_dir(out_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(out_dir)?;
    for entry in std::fs::read_dir(out_dir)? {
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with("acct_") || name.starts_with("program_") {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// Dump `keys` (minus `skip`: the fork initialises those itself) and the upgradeable programs
/// that own them or that they are, into `out_dir`.
async fn dump(ctx: &Ctx, out_dir: &Path, keys: &[Pubkey], skip: &[Pubkey]) -> Result<Dump> {
    let chain = &ctx.chain;
    let own = ctx.cfg.program_id;
    let mut programs: Vec<Pubkey> = Vec::new();
    let mut missing = Vec::new();
    let mut accounts = 0usize;
    let mut slot = 0u64;
    let mut wanted: Vec<Pubkey> = Vec::new();
    for k in keys {
        if !skip.contains(k) && !wanted.contains(k) {
            wanted.push(*k);
        }
    }
    let cfg = RpcAccountInfoConfig { commitment: Some(CommitmentConfig::confirmed()), ..Default::default() };
    for chunk in wanted.chunks(50) {
        let res = chain.rpc.get_multiple_accounts_with_config(chunk, cfg.clone()).await.context("accounts")?;
        slot = res.context.slot;
        for (k, acc) in chunk.iter().zip(res.value) {
            match acc {
                None => missing.push(*k),
                Some(a) => {
                    if a.executable {
                        if !programs.contains(k) && *k != own && !is_native_program(k) {
                            programs.push(*k);
                        }
                        continue;
                    }
                    if !programs.contains(&a.owner) && a.owner != own && !is_native_program(&a.owner) {
                        programs.push(a.owner);
                    }
                    let f = json!({ "pubkey": k.to_string(), "owner": a.owner.to_string(), "lamports": a.lamports, "executable": false, "data_base64": b64(&a.data) });
                    std::fs::write(out_dir.join(format!("acct_{k}.json")), serde_json::to_string(&f)?)?;
                    accounts += 1;
                }
            }
        }
    }
    let mut program_files = Vec::new();
    for p in &programs {
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
    Ok(Dump { accounts, programs: program_files, missing, slot })
}

async fn block_time(ctx: &Ctx, slot: u64) -> i64 {
    ctx.chain.rpc.get_block_time(slot).await.unwrap_or_else(|_| chrono::Utc::now().timestamp())
}

/// Print a simulation outcome: success tail, or the first line plus the log tail.
fn report(shape: &str, res: Result<Vec<String>>) -> bool {
    match res {
        Ok(logs) => {
            println!("simulation [{shape}] succeeded ({} log lines)", logs.len());
            for l in logs.iter().rev().take(6).rev() {
                println!("  {l}");
            }
            true
        }
        Err(e) => {
            let text = format!("{e:#}");
            let too_large = text.contains("too large");
            println!(
                "simulation [{shape}] failed{}:",
                if too_large { " to fit in a packet without the keeper's per-vault lookup table" } else { "" }
            );
            let lines: Vec<&str> = text.lines().collect();
            if let Some(first) = lines.first() {
                println!("  {first}");
            }
            for l in lines.iter().skip(1).rev().take(8).rev() {
                println!("  {l}");
            }
            false
        }
    }
}

struct VaultRefs {
    vc: VaultCfg,
    vault: Pubkey,
    custody: Pubkey,
    buffer: Pubkey,
}

fn vault_refs(ctx: &Ctx, symbol: &str) -> Result<VaultRefs> {
    let vc = ctx.cfg.vaults.iter().find(|v| v.symbol.eq_ignore_ascii_case(symbol)).ok_or_else(|| anyhow!("no vault {symbol} in config"))?.clone();
    let vault = ctx.chain.pdas().vault(&vc.mint);
    Ok(VaultRefs { vc, vault, custody: ctx.chain.pdas().stock_custody(&vault), buffer: ctx.chain.pdas().usdc_buffer(&vault) })
}

async fn kamino_block(ctx: &Ctx, r: &VaultRefs) -> Result<Vec<AccountMeta>> {
    let cfg = &ctx.cfg;
    let stock_reserve = r.vc.kamino_reserve.ok_or_else(|| anyhow!("{}: kamino_reserve not configured", r.vc.symbol))?;
    if cfg.kamino_reserve == Pubkey::default() {
        return Err(anyhow!("kamino_reserve (USDC) not configured"));
    }
    let accs = ctx.chain.rpc.get_multiple_accounts(&[stock_reserve, cfg.kamino_reserve]).await.context("reserves")?;
    let stock = ReserveInfo::parse(&accs[0].as_ref().ok_or_else(|| anyhow!("stock reserve missing"))?.data)?;
    let usdc = ReserveInfo::parse(&accs[1].as_ref().ok_or_else(|| anyhow!("usdc reserve missing"))?.data)?;
    KaminoBlock::metas(&r.vault, &r.vc.mint, &r.custody, &r.buffer, &stock_reserve, &stock, &cfg.kamino_reserve, &usdc)
}

/// Kamino publishes one address lookup table per market (`/v2/kamino-market/<market>`).
async fn kamino_market_table(http: &reqwest::Client, api: &str, market: &str) -> Result<Pubkey> {
    let v: serde_json::Value = http.get(format!("{api}/v2/kamino-market/{market}")).send().await?.error_for_status()?.json().await?;
    v["lookupTable"].as_str().ok_or_else(|| anyhow!("no lookupTable in market metadata"))?.parse().context("lookupTable")
}

async fn phoenix_market(ctx: &Ctx, vc: &VaultCfg) -> Result<(PhoenixKeys, venue::PhoenixMarket)> {
    let keys = venue::fetch_phoenix_keys(&ctx.http, &ctx.cfg.phoenix_api_url).await?;
    let market = keys.market(&vc.phoenix_market).ok_or_else(|| anyhow!("Phoenix market {} not listed", vc.phoenix_market))?.clone();
    Ok((keys, market))
}

fn route_json(r: &JupiterRoute, amount: u64) -> serde_json::Value {
    json!({
        "amount": amount,
        "quoted_out_amount": r.quoted_out_amount,
        "data_base64": b64(&r.data),
        "block": metas_json(&r.block),
        "lookup_tables": r.lookup_tables.iter().map(|k| k.to_string()).collect::<Vec<_>>(),
    })
}

/// `venues prove-jupiter`: live routes both ways, the real-build `wind_step(1)` simulated on the
/// configured RPC, and both routes' accounts dumped for the fork.
pub async fn prove_jupiter(ctx: &Ctx, symbol: &str, amount: u64, dexes: Option<&str>, out_dir: &Path) -> Result<()> {
    let cfg: &Config = &ctx.cfg;
    let r = vault_refs(ctx, symbol)?;
    let vc = &r.vc;
    let chain = &ctx.chain;
    let base = jupiter_base_url(&cfg.jupiter_price_url);

    // 1. Live routes both ways. The reverse is quoted at roughly the size the fork sells back
    //    (100 × the forward output) so its pools are the ones a real unwind would use.
    if let Some(d) = dexes {
        println!("routes restricted to {d} (the fork replays AMM state as dumped; quote-time-bound market makers are excluded)");
    }
    let fwd = jupiter_route_with(&ctx.http, &base, &cfg.usdc_mint, &vc.mint, amount, 50, &r.vault, &r.buffer, &r.custody, dexes).await.context("USDC→stock route")?;
    println!("USDC→{}x: {amount} in, quoted out {}, {} accounts, {} bytes, tables {:?}", vc.symbol, fwd.quoted_out_amount, fwd.block.len(), fwd.data.len(), fwd.lookup_tables);
    let rev_amount = fwd.quoted_out_amount.max(1) * 100;
    let rev = jupiter_route_with(&ctx.http, &base, &vc.mint, &cfg.usdc_mint, rev_amount, 50, &r.vault, &r.custody, &r.buffer, dexes).await.context("stock→USDC route")?;
    println!("{}x→USDC: {rev_amount} in, quoted out {}, {} accounts, {} bytes, tables {:?}", vc.symbol, rev.quoted_out_amount, rev.block.len(), rev.data.len(), rev.lookup_tables);

    // 2. The full wind_step(1) instruction as the real build expects it.
    let kamino = kamino_block(ctx, &r).await?;
    let (keys, market) = phoenix_market(ctx, vc).await?;
    let phoenix = venue::phoenix_block(&r.vault, &cfg.usdc_mint, &r.buffer, &keys, &market);
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
        chain.ix.wind_step(&r.vault, 1, &args)
    };
    let ix = build(BLOCK_KAMINO | BLOCK_PHOENIX | BLOCK_JUPITER, true);
    let step1 = build(BLOCK_KAMINO | BLOCK_JUPITER, false);
    println!(
        "wind_step(1): {} accounts ({} kamino + {} phoenix + {} jupiter), venue_data {} bytes, swap {:?}; unique locks: {} with all three blocks, {} with Kamino + Jupiter (mainnet max {})",
        ix.accounts.len(),
        kamino.len(),
        phoenix.len(),
        fwd.block.len(),
        fwd.data.len() + 43,
        Swap::UsdcToStock(amount),
        crate::chain::unique_accounts(std::slice::from_ref(&ix), &chain.keeper()),
        crate::chain::unique_accounts(std::slice::from_ref(&step1), &chain.keeper()),
        crate::chain::MAX_TX_ACCOUNT_LOCKS
    );

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
    let attempts = [(ix.clone(), "kamino+phoenix+jupiter"), (step1, "kamino+jupiter (what step 1 consumes)")];
    for (ix, shape) in attempts {
        let res = chain.simulate_ixs(vec![ix], &tables).await;
        let too_large = res.as_ref().err().map(|e| format!("{e:#}").contains("too large")).unwrap_or(false);
        report(shape, res);
        if !too_large {
            break;
        }
    }

    // 4. Dump fixtures: both routes' accounts plus the AMM programs they invoke.
    clear_dir(out_dir)?;
    let mut keys_to_dump: Vec<Pubkey> = fwd.block.iter().map(|m| m.pubkey).collect();
    keys_to_dump.extend(rev.block.iter().map(|m| m.pubkey));
    let d = dump(ctx, out_dir, &keys_to_dump, &[r.vault, r.custody, r.buffer]).await?;
    let scenario = json!({
        "vault_symbol": vc.symbol,
        "vault": r.vault.to_string(),
        "input_mint": cfg.usdc_mint.to_string(),
        "output_mint": vc.mint.to_string(),
        "dexes": dexes,
        "forward": route_json(&fwd, amount),
        "reverse": route_json(&rev, rev_amount),
        // Kept for readers of the first fixture format.
        "amount": amount,
        "quoted_out_amount": fwd.quoted_out_amount,
        "data_base64": b64(&fwd.data),
        "block": metas_json(&fwd.block),
        "lookup_tables": fwd.lookup_tables.iter().map(|k| k.to_string()).collect::<Vec<_>>(),
        "programs": d.programs,
        "missing_accounts": d.missing.iter().map(|k| k.to_string()).collect::<Vec<_>>(),
        "dump_slot": d.slot,
        "dump_ts": block_time(ctx, d.slot).await,
    });
    std::fs::write(out_dir.join("scenario.json"), serde_json::to_string_pretty(&scenario)?)?;
    println!("fixtures: {} accounts, {} programs, {} missing (PDAs that do not exist yet) → {}", d.accounts, scenario["programs"].as_array().map(|a| a.len()).unwrap_or(0), d.missing.len(), out_dir.display());
    Ok(())
}

/// `venues prove-phoenix`: the vault's trader registration and collateral-account creation
/// simulated on the configured RPC (these are the only Phoenix transactions the keeper sends
/// outside a crank), the real-build `wind_step(3)` shape simulated, and every Phoenix / Ember
/// account the block references dumped for the fork.
pub async fn prove_phoenix(ctx: &Ctx, symbol: &str, out_dir: &Path) -> Result<()> {
    let cfg: &Config = &ctx.cfg;
    let r = vault_refs(ctx, symbol)?;
    let vc = &r.vc;
    let chain = &ctx.chain;
    let (keys, market) = phoenix_market(ctx, vc).await?;
    let trader_account = venue::trader_account(&r.vault);
    let trader_token = associated_token_address(&r.vault, &keys.canonical_mint, &TOKEN_PROGRAM_ID);
    let base_lot_size = venue::base_lot_size(vc.stock_decimals, market.base_lots_decimals);
    println!(
        "{}: Phoenix market {} orderbook {} spline {} base_lots_decimals {} → base_lot_size {} ({} decimals)",
        vc.symbol, market.symbol, market.orderbook, market.spline, market.base_lots_decimals, base_lot_size, vc.stock_decimals
    );
    println!("trader PDA {trader_account} (index {}, subaccount {}), collateral ATA {trader_token}", venue::TRADER_PDA_INDEX, venue::TRADER_SUBACCOUNT);
    let registered = chain.rpc.get_account(&trader_account).await.map(|a| a.data.len() > 8).unwrap_or(false);
    println!("trader registered on mainnet: {registered}");

    // 1. Setup transactions: collateral ATA (idempotent), then Phoenix's own register +
    //    delegated-onboarding instructions for the vault (the onboarder's signature is added by
    //    Phoenix at send time; the simulation runs without signature verification).
    let keeper = chain.keeper();
    let ata_ix = venue::create_ata_idempotent(&keeper, &r.vault, &keys.canonical_mint, &TOKEN_PROGRAM_ID);
    let built = venue::phoenix_register_ixs(ctx, &r.vault, &keeper).await?;
    println!(
        "Phoenix build-register-ixs: {} instructions, onboarder {}, trader {}, include_register_trader {}",
        built.instructions.len(),
        built.trader_onboarder,
        built.trader_pda,
        built.include_register_trader
    );
    if built.trader_pda != trader_account {
        return Err(anyhow!("Phoenix derived trader {} but the keeper expects {trader_account}", built.trader_pda));
    }
    let local = venue::register_trader_ix(&keeper, &r.vault, &trader_account)?;
    if let Some(api_reg) = built.instructions.iter().find(|ix| ix.data == local.data) {
        println!("register_trader from the API matches the keeper's builder ({} accounts)", api_reg.accounts.len());
    }
    let mut setup = vec![ata_ix.clone()];
    setup.extend(built.instructions.iter().cloned());
    report("create collateral ATA + register_trader + onboard_trader_delegated", chain.simulate_ixs(setup, &[]).await);

    // 2. The wind_step(3) shape (Kamino + Phoenix blocks, the short order) on the deployed program.
    let kamino = kamino_block(ctx, &r).await?;
    let phoenix = venue::phoenix_block(&r.vault, &cfg.usdc_mint, &r.buffer, &keys, &market);
    let slot = chain.slot().await?;
    let data = VenueData {
        blocks: BLOCK_KAMINO | BLOCK_PHOENIX,
        phoenix_gti: keys.global_trader_index.len() as u8,
        phoenix_atb: keys.active_trader_buffer.len() as u8,
        base_lot_size,
        last_valid_slot: slot + venue::ORDER_VALID_SLOTS,
        client_order_id: chrono::Utc::now().timestamp() as u64,
        ..Default::default()
    };
    let mut remaining = kamino.clone();
    remaining.extend(phoenix.iter().cloned());
    let args = VenueArgs::from_data(&data, remaining, Vec::new());
    let ix = chain.ix.wind_step(&r.vault, 3, &args);
    println!(
        "wind_step(3): {} accounts ({} kamino + {} phoenix), venue_data {} bytes, {} unique account locks (mainnet max {})",
        ix.accounts.len(),
        kamino.len(),
        phoenix.len(),
        args.data.len(),
        crate::chain::unique_accounts(std::slice::from_ref(&ix), &keeper),
        crate::chain::MAX_TX_ACCOUNT_LOCKS
    );
    let mut tables = Vec::new();
    match kamino_market_table(&ctx.http, &cfg.kamino_api_url, &cfg.kamino_market).await {
        Ok(t) => tables.push(t),
        Err(e) => println!("Kamino market lookup table unavailable: {e:#}"),
    }
    report("kamino+phoenix wind_step(3)", chain.simulate_ixs(vec![ix], &tables).await);

    // 3. Dump the block's accounts and the Phoenix + Ember programs.
    clear_dir(out_dir)?;
    let mut keys_to_dump: Vec<Pubkey> = phoenix.iter().map(|m| m.pubkey).collect();
    for ix in &built.instructions {
        keys_to_dump.extend(ix.accounts.iter().map(|m| m.pubkey));
    }
    let d = dump(ctx, out_dir, &keys_to_dump, &[r.vault, r.custody, r.buffer, trader_account, trader_token, keeper, built.trader_onboarder]).await?;
    let scenario = json!({
        "vault_symbol": vc.symbol,
        "vault": r.vault.to_string(),
        "trader_account": trader_account.to_string(),
        "trader_token_account": trader_token.to_string(),
        "canonical_mint": keys.canonical_mint.to_string(),
        "market": { "symbol": market.symbol, "orderbook": market.orderbook.to_string(), "spline": market.spline.to_string(), "base_lots_decimals": market.base_lots_decimals },
        "base_lot_size": base_lot_size,
        "phoenix_gti": keys.global_trader_index.len(),
        "phoenix_atb": keys.active_trader_buffer.len(),
        "block": metas_json(&phoenix),
        "create_ata_ix": ix_json(&ata_ix),
        "onboarding_ixs": built.instructions.iter().map(ix_json).collect::<Vec<_>>(),
        "trader_onboarder": built.trader_onboarder.to_string(),
        "keeper": keeper.to_string(),
        "programs": d.programs,
        "missing_accounts": d.missing.iter().map(|k| k.to_string()).collect::<Vec<_>>(),
        "dump_slot": d.slot,
        "dump_ts": block_time(ctx, d.slot).await,
    });
    std::fs::write(out_dir.join("scenario.json"), serde_json::to_string_pretty(&scenario)?)?;
    println!("fixtures: {} accounts, {} programs, {} missing → {}", d.accounts, scenario["programs"].as_array().map(|a| a.len()).unwrap_or(0), d.missing.len(), out_dir.display());
    Ok(())
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
