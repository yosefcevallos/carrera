//! Keeper-owned address lookup tables, one per vault, holding the Kamino and Phoenix block
//! accounts so a venue crank fits in a v0 transaction next to a Jupiter route.
//!
//! The table address is persisted in `<state_dir>/alt-<SYMBOL>.json` so restarts reuse it.
//! `ensure` creates the table on first use and extends it with any address not yet present;
//! new entries only become usable one slot after the extension confirms, so `ensure` waits for
//! the slot to advance before returning.

use crate::chain::Chain;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use solana_sdk::{
    address_lookup_table::{instruction as alt_ix, state::AddressLookupTable},
    pubkey::Pubkey,
    signature::Signer,
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Serialize, Deserialize)]
struct AltRecord {
    table: String,
}

pub fn record_path(state_dir: &Path, symbol: &str) -> PathBuf {
    state_dir.join(format!("alt-{symbol}.json"))
}

fn load(state_dir: &Path, symbol: &str) -> Option<Pubkey> {
    let text = std::fs::read_to_string(record_path(state_dir, symbol)).ok()?;
    let rec: AltRecord = serde_json::from_str(&text).ok()?;
    rec.table.parse().ok()
}

fn store(state_dir: &Path, symbol: &str, table: &Pubkey) -> Result<()> {
    std::fs::create_dir_all(state_dir).ok();
    let tmp = record_path(state_dir, symbol).with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string(&AltRecord { table: table.to_string() })?)?;
    std::fs::rename(&tmp, record_path(state_dir, symbol))?;
    Ok(())
}

/// Return the vault's table, creating it and adding any missing `addresses` first.
/// Idempotent: existing entries are never re-added; an empty `addresses` just returns the table.
pub async fn ensure(chain: &Chain, state_dir: &Path, symbol: &str, addresses: &[Pubkey]) -> Result<Pubkey> {
    let payer = chain.keeper();
    let (table, existing) = match load(state_dir, symbol) {
        Some(t) => match chain.rpc.get_account(&t).await {
            Ok(acc) => {
                let parsed = AddressLookupTable::deserialize(&acc.data).map_err(|e| anyhow!("lookup table {t}: {e}"))?;
                (t, parsed.addresses.to_vec())
            }
            Err(_) => create(chain, state_dir, symbol).await?,
        },
        None => create(chain, state_dir, symbol).await?,
    };
    let missing: Vec<Pubkey> = {
        let mut seen = existing.clone();
        let mut m = Vec::new();
        for a in addresses {
            if !seen.contains(a) {
                seen.push(*a);
                m.push(*a);
            }
        }
        m
    };
    if missing.is_empty() {
        return Ok(table);
    }
    let before = chain.slot().await?;
    for chunk in missing.chunks(20) {
        let ix = alt_ix::extend_lookup_table(table, payer, Some(payer), chunk.to_vec());
        chain.send_ixs(&format!("{symbol} alt extend +{}", chunk.len()), vec![ix], &[]).await?;
    }
    // Entries added in slot S are usable from slot S+1, and a load-balanced RPC may lag behind the
    // node that confirmed the extension: poll the table (finalized) until every address is visible
    // and the chain has moved past the extension slot.
    let wanted = existing.len() + missing.len();
    let mut visible = false;
    for _ in 0..60 {
        let acc = chain
            .rpc
            .get_account_with_commitment(&table, solana_sdk::commitment_config::CommitmentConfig::finalized())
            .await
            .ok()
            .and_then(|r| r.value);
        if let Some(acc) = acc {
            if let Ok(parsed) = AddressLookupTable::deserialize(&acc.data) {
                let last_ext = parsed.meta.last_extended_slot;
                if parsed.addresses.len() >= wanted && chain.slot().await? > last_ext + 1 && chain.slot().await? > before + 1 {
                    visible = true;
                    break;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    if !visible {
        return Err(anyhow!("{symbol}: lookup table {table} did not become fully visible within 30 s"));
    }
    tracing::info!("{symbol}: lookup table {table} now holds {} addresses", existing.len() + missing.len());
    Ok(table)
}

async fn create(chain: &Chain, state_dir: &Path, symbol: &str) -> Result<(Pubkey, Vec<Pubkey>)> {
    let payer = chain.payer.pubkey();
    // The lookup-table program only accepts a slot present in SlotHashes on the executing node; the
    // latest processed slot from a load-balanced RPC is often ahead of it, so use a finalized slot.
    let recent = chain
        .rpc
        .get_slot_with_commitment(solana_sdk::commitment_config::CommitmentConfig::finalized())
        .await
        .context("finalized slot")?;
    let (ix, table) = alt_ix::create_lookup_table(payer, payer, recent);
    chain.send_ixs(&format!("{symbol} alt create {table}"), vec![ix], &[]).await?;
    store(state_dir, symbol, &table)?;
    Ok((table, Vec::new()))
}

/// Where per-vault state lives: the directory of the lease file.
pub fn state_dir_from_lease(lease_path: &Path) -> PathBuf {
    lease_path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_round_trips() {
        let dir = std::env::temp_dir().join(format!("carrera-alt-test-{}", std::process::id()));
        let table = Pubkey::new_unique();
        store(&dir, "TSLA", &table).unwrap();
        assert_eq!(load(&dir, "TSLA"), Some(table));
        assert_eq!(load(&dir, "NVDA"), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn state_dir_is_lease_parent() {
        assert_eq!(state_dir_from_lease(Path::new("/var/lib/carrera/keeper.lease")), PathBuf::from("/var/lib/carrera"));
    }
}
