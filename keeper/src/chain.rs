//! RPC access: read program accounts, sign and send single-instruction transactions.

use crate::{
    accounts::{decode, ExitEpoch, OverlayVault, Registry},
    ix::{IxBuilder, Pdas},
};
use anyhow::{anyhow, Context, Result};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    instruction::Instruction,
    pubkey::Pubkey,
    signature::{read_keypair_file, Keypair, Signature, Signer},
    transaction::Transaction,
};
use std::path::Path;

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

    /// Sign and send one instruction, waiting for confirmation. Errors are returned,
    /// not swallowed; loops decide whether to continue.
    /// A stale blockhash at preflight ("Blockhash not found" / block height exceeded)
    /// is retried up to two more times with a fresh blockhash; every other error is
    /// returned as is.
    pub async fn send(&self, label: &str, ix: Instruction) -> Result<Signature> {
        const ATTEMPTS: usize = 3;
        let mut last = None;
        for attempt in 1..=ATTEMPTS {
            let bh = self.rpc.get_latest_blockhash().await.context("blockhash")?;
            let tx = Transaction::new_signed_with_payer(std::slice::from_ref(&ix), Some(&self.payer.pubkey()), &[&self.payer], bh);
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
