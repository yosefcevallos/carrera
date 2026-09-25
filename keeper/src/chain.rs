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
        const ATTEMPTS: usize = 3;
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
            match self.rpc.send_and_confirm_transaction(&tx).await {
                Ok(sig) => {
                    tracing::info!(%sig, "{label}");
                    return Ok(sig);
                }
                Err(e) => {
                    let msg = e.to_string();
                    let stale = msg.contains("Blockhash not found") || msg.contains("BlockhashNotFound") || msg.contains("block height exceeded");
                    if stale && attempt < ATTEMPTS {
                        tracing::warn!("{label}: stale blockhash at preflight, retrying ({attempt}/{ATTEMPTS})");
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
        let tx = VersionedTransaction::try_new(VersionedMessage::V0(msg), &[&self.payer]).map_err(|e| anyhow!("signing: {e}"))?;
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

    pub async fn slot(&self) -> Result<u64> {
        self.rpc.get_slot().await.context("slot")
    }

    pub async fn sol_balance(&self) -> Result<f64> {
        let lamports = self.rpc.get_balance(&self.payer.pubkey()).await.context("balance")?;
        Ok(lamports as f64 / 1_000_000_000.0)
    }
}
