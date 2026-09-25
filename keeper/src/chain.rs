//! RPC access: read program accounts, sign and send versioned (v0) transactions.
//!
//! Every send is a v0 transaction with a compute-budget prefix (priority fee, CU limit) and
//! optional address lookup tables: Jupiter's route tables plus the keeper-owned per-vault table
//! that holds the Kamino and Phoenix block accounts (see `alt.rs`). Legacy-only paths are gone;
//! a plain crank without tables is simply a v0 message with an empty table list.

use crate::{
    accounts::{decode, ExitEpoch, OverlayVault, Registry},
    ix::{IxBuilder, Pdas},
    venue_accounts::VenueArgs,
};
use anyhow::{anyhow, Context, Result};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    address_lookup_table::state::AddressLookupTable,
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
    message::{v0, AddressLookupTableAccount, VersionedMessage},
    packet::PACKET_DATA_SIZE,
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signature, Signer},
    transaction::VersionedTransaction,
};
use std::path::Path;

/// Priority fee attached to every transaction (micro-lamports per CU).
pub const PRIORITY_FEE_MICRO_LAMPORTS: u64 = 50_000;
/// CU limit for plain cranks (no venue blocks).
pub const CU_LIMIT_PLAIN: u32 = 400_000;
/// CU limit for cranks carrying venue blocks (Kamino refreshes + a Jupiter route + Phoenix).
pub const CU_LIMIT_VENUE: u32 = 1_400_000;
/// Unique accounts one transaction may lock on mainnet (`increase_tx_account_lock_limit` is not
/// active there; lookup tables do not raise it).
pub const MAX_TX_ACCOUNT_LOCKS: usize = 64;

/// Refuse a signed transaction that would not fit a packet, naming the serialised size and
/// where the keys are (static vs looked up through tables).
pub fn check_packet_size(label: &str, tx: &VersionedTransaction) -> Result<()> {
    let size = bincode::serialize(tx).map(|b| b.len()).unwrap_or(usize::MAX);
    if size <= PACKET_DATA_SIZE {
        return Ok(());
    }
    let keys = tx.message.static_account_keys().len();
    let lookups = tx.message.address_table_lookups().map(|l| l.to_vec()).unwrap_or_default();
    let looked_up: usize = lookups.iter().map(|t| t.writable_indexes.len() + t.readonly_indexes.len()).sum();
    Err(anyhow!(
        "{label}: serialised transaction is {size} bytes, over the {PACKET_DATA_SIZE}-byte packet limit ({keys} static keys, {looked_up} looked up through {} tables)",
        lookups.len()
    ))
}

/// Unique account keys a transaction built from `ixs` would lock: payer, compute-budget
/// program, every program id and every account meta.
pub fn unique_accounts(ixs: &[Instruction], payer: &Pubkey) -> usize {
    let mut keys: Vec<Pubkey> = vec![*payer, solana_sdk::compute_budget::ID];
    for ix in ixs {
        if !keys.contains(&ix.program_id) {
            keys.push(ix.program_id);
        }
        for m in &ix.accounts {
            if !keys.contains(&m.pubkey) {
                keys.push(m.pubkey);
            }
        }
    }
    keys.len()
}

pub struct Chain {
    pub rpc: RpcClient,
    pub payer: Keypair,
    pub ix: IxBuilder,
}

impl Chain {
    pub fn new(rpc_url: &str, keypair_path: &Path, program_id: Pubkey) -> Result<Self> {
        let payer = read_keypair_file(keypair_path)
            .map_err(|e| anyhow!("reading keypair {}: {e}", keypair_path.display()))?;
        let ix = IxBuilder::new(program_id, payer.pubkey());
        Ok(Self { rpc: RpcClient::new_with_commitment(rpc_url.to_string(), CommitmentConfig::confirmed()), payer, ix })
    }

    pub fn pdas(&self) -> &Pdas {
        &self.ix.pdas
    }

    pub fn keeper(&self) -> Pubkey {
        self.payer.pubkey()
    }

    /// Load and deserialise address lookup tables; a table that does not exist is an error
    /// (a missing table would silently make the compile fail on account count).
    pub async fn lookup_tables(&self, keys: &[Pubkey]) -> Result<Vec<AddressLookupTableAccount>> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let accounts = self.rpc.get_multiple_accounts(keys).await.context("lookup tables")?;
        let mut out = Vec::with_capacity(keys.len());
        for (key, acc) in keys.iter().zip(accounts) {
            let acc = acc.ok_or_else(|| anyhow!("lookup table {key} not found"))?;
            let table = AddressLookupTable::deserialize(&acc.data).map_err(|e| anyhow!("lookup table {key}: {e}"))?;
            out.push(AddressLookupTableAccount { key: *key, addresses: table.addresses.to_vec() });
        }
        Ok(out)
    }

    /// Sign and send one crank as a v0 transaction, waiting for confirmation. Venue cranks pass
    /// their lookup tables; the CU limit is raised when remaining accounts are present.
    pub async fn send(&self, label: &str, ix: Instruction) -> Result<Signature> {
        self.send_ixs(label, vec![ix], &[]).await
    }

    /// Send a venue crank: the instruction already carries the remaining-account blocks, the
    /// `VenueArgs` carries the Jupiter route tables; `extra_tables` is the keeper's per-vault table.
    pub async fn send_venue(&self, label: &str, ix: Instruction, venue: &VenueArgs, extra_tables: &[Pubkey]) -> Result<Signature> {
        let mut tables = venue.lookup_tables.clone();
        tables.extend(extra_tables.iter().copied());
        tables.dedup();
        self.send_ixs(label, vec![ix], &tables).await
    }

    /// Build, sign and send a v0 transaction. A stale blockhash at preflight ("Blockhash not
    /// found" / block height exceeded) is retried up to two more times with a fresh blockhash;
    /// every other error is returned as is.
    pub async fn send_ixs(&self, label: &str, ixs: Vec<Instruction>, tables: &[Pubkey]) -> Result<Signature> {
        const ATTEMPTS: usize = 6;
        let locks = unique_accounts(&ixs, &self.payer.pubkey());
        if locks > MAX_TX_ACCOUNT_LOCKS {
            return Err(anyhow!("{label}: {locks} unique accounts exceed the {MAX_TX_ACCOUNT_LOCKS} account locks a mainnet transaction may hold"));
        }
        let venue_tx = ixs.iter().any(|i| i.accounts.len() > 16);
        let cu = if venue_tx { CU_LIMIT_VENUE } else { CU_LIMIT_PLAIN };
        let mut all = Vec::with_capacity(ixs.len() + 2);
        all.push(ComputeBudgetInstruction::set_compute_unit_limit(cu));
        all.push(ComputeBudgetInstruction::set_compute_unit_price(PRIORITY_FEE_MICRO_LAMPORTS));
        all.extend(ixs);
        let alts = self.lookup_tables(tables).await?;
        let mut last = None;
        for attempt in 1..=ATTEMPTS {
            let bh = self.rpc.get_latest_blockhash().await.context("blockhash")?;
            let msg = v0::Message::try_compile(&self.payer.pubkey(), &all, &alts, bh)
                .map_err(|e| anyhow!("{label}: compiling v0 message: {e}"))?;
            let tx = VersionedTransaction::try_new(VersionedMessage::V0(msg), &[&self.payer])
                .map_err(|e| anyhow!("{label}: signing: {e}"))?;
            check_packet_size(label, &tx)?;
            match self.rpc.send_and_confirm_transaction(&tx).await {
                Ok(sig) => {
                    tracing::info!(%sig, "{label}");
                    return Ok(sig);
                }
                Err(e) => {
                    let msg = e.to_string();
                    let stale = msg.contains("Blockhash not found") || msg.contains("BlockhashNotFound") || msg.contains("block height exceeded");
                    // A lookup-table entry not yet visible on the preflight node.
                    let alt_lag = msg.contains("address table lookup uses an invalid index") || msg.contains("invalid index");
                    if (stale || alt_lag) && attempt < ATTEMPTS {
                        tracing::warn!("{label}: {} at preflight, retrying ({attempt}/{ATTEMPTS})", if alt_lag { "lookup table not yet visible" } else { "stale blockhash" });
                        if alt_lag {
                            tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
                        }
                        last = Some(e);
                        continue;
                    }
                    return Err(anyhow::Error::new(e).context(format!("{label} failed")));
                }
            }
        }
        Err(anyhow::Error::new(last.expect("retried")).context(format!("{label} failed after {ATTEMPTS} attempts")))
    }

    /// Simulate a v0 transaction without sending it (signature verification off, fresh
    /// blockhash). Returns the simulation logs, or the error with logs attached.
    pub async fn simulate_ixs(&self, ixs: Vec<Instruction>, tables: &[Pubkey]) -> Result<Vec<String>> {
        use solana_client::rpc_config::RpcSimulateTransactionConfig;
        let mut all = vec![ComputeBudgetInstruction::set_compute_unit_limit(CU_LIMIT_VENUE)];
        all.extend(ixs);
        let alts = self.lookup_tables(tables).await?;
        let bh = self.rpc.get_latest_blockhash().await.context("blockhash")?;
        let msg = v0::Message::try_compile(&self.payer.pubkey(), &all, &alts, bh).map_err(|e| anyhow!("compiling v0 message: {e}"))?;
        // Partially signed: only the payer signs; other signer metas (e.g. Phoenix's onboarder)
        // stay blank, which `sig_verify: false` accepts.
        let message = VersionedMessage::V0(msg);
        let mut signatures = vec![Signature::default(); message.header().num_required_signatures as usize];
        signatures[0] = self.payer.sign_message(&message.serialize());
        let tx = VersionedTransaction { signatures, message };
        let cfg = RpcSimulateTransactionConfig { sig_verify: false, replace_recent_blockhash: true, commitment: Some(CommitmentConfig::confirmed()), ..Default::default() };
        let res = self.rpc.simulate_transaction_with_config(&tx, cfg).await.context("simulate")?;
        let logs = res.value.logs.unwrap_or_default();
        match res.value.err {
            None => Ok(logs),
            Some(e) => Err(anyhow!("simulation failed: {e:?}\n{}", logs.join("\n"))),
        }
    }

    /// Like `send` but logs the error and returns whether it succeeded.
    pub async fn try_send(&self, label: &str, ix: Instruction) -> bool {
        match self.send(label, ix).await {
            Ok(_) => true,
            Err(e) => {
                tracing::warn!("{e:#}");
                false
            }
        }
    }

    /// Like `send_venue` but logs the error and returns whether it succeeded.
    pub async fn try_send_venue(&self, label: &str, ix: Instruction, venue: &VenueArgs, extra_tables: &[Pubkey]) -> bool {
        match self.send_venue(label, ix, venue, extra_tables).await {
            Ok(_) => true,
            Err(e) => {
                tracing::warn!("{e:#}");
                false
            }
        }
    }

    pub async fn registry(&self) -> Result<Registry> {
        let acc = self.rpc.get_account(&self.pdas().registry()).await.context("registry account")?;
        decode("Registry", &acc.data)
    }

    pub async fn vault(&self, xstock_mint: &Pubkey) -> Result<OverlayVault> {
        let acc = self.rpc.get_account(&self.pdas().vault(xstock_mint)).await.context("vault account")?;
        decode("OverlayVault", &acc.data)
    }

    pub async fn epoch(&self, vault: &Pubkey, id: u64) -> Result<Option<ExitEpoch>> {
        let accs = self.rpc.get_multiple_accounts(&[self.pdas().exit_epoch(vault, id)]).await.context("epoch account")?;
        match accs.into_iter().next().flatten() {
            Some(a) => Ok(Some(decode("ExitEpoch", &a.data)?)),
            None => Ok(None),
        }
    }

    /// Batch fetch of exit epochs `ids` for `vault`, in the order given.
    pub async fn epochs(&self, vault: &Pubkey, ids: &[u64]) -> Result<Vec<(u64, Option<ExitEpoch>)>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let keys: Vec<Pubkey> = ids.iter().map(|id| self.pdas().exit_epoch(vault, *id)).collect();
        let accs = self.rpc.get_multiple_accounts(&keys).await.context("epoch accounts")?;
        ids.iter()
            .zip(accs)
            .map(|(id, a)| Ok((*id, match a { Some(a) => Some(decode("ExitEpoch", &a.data)?), None => None })))
            .collect()
    }

    pub async fn slot(&self) -> Result<u64> {
        self.rpc.get_slot().await.context("slot")
    }

    pub async fn sol_balance(&self) -> Result<f64> {
        let lamports = self.rpc.get_balance(&self.payer.pubkey()).await.context("balance")?;
        Ok(lamports as f64 / 1_000_000_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::{hash::Hash, instruction::AccountMeta};

    fn tx_with(n_accounts: usize) -> VersionedTransaction {
        let payer = Keypair::new();
        let metas: Vec<AccountMeta> = (0..n_accounts).map(|_| AccountMeta::new(Pubkey::new_unique(), false)).collect();
        let ix = Instruction { program_id: Pubkey::new_unique(), accounts: metas, data: vec![0; 16] };
        let msg = v0::Message::try_compile(&payer.pubkey(), &[ix], &[], Hash::default()).unwrap();
        VersionedTransaction::try_new(VersionedMessage::V0(msg), &[&payer]).unwrap()
    }

    #[test]
    fn packet_size_check_names_the_size() {
        assert!(check_packet_size("small", &tx_with(10)).is_ok());
        // 25 accounts + user metadata with raw keys is what init_kamino_obligation needs; 40
        // raw keys is well over 1232 bytes.
        let err = check_packet_size("init_kamino_obligation", &tx_with(40)).unwrap_err().to_string();
        assert!(err.contains("over the 1232-byte packet limit") && err.contains("42 static keys"), "{err}");
    }
}
