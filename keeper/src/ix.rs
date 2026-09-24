//! Instruction builders and PDA derivation for `carrera_overlay`, per docs/CONTRACT.md.
//! No dependency on the program crate: discriminators are sha256("global:<name>")[..8],
//! args are Borsh in contract order, accounts follow the contract's row order with the
//! signer first.

use borsh::BorshSerialize;
use sha2::{Digest, Sha256};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey,
    pubkey::Pubkey,
};

pub const TOKEN_PROGRAM_ID: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");

pub fn discriminator(name: &str) -> [u8; 8] {
    let h = Sha256::digest(format!("global:{name}").as_bytes());
    let mut d = [0u8; 8];
    d.copy_from_slice(&h[..8]);
    d
}

pub fn account_discriminator(name: &str) -> [u8; 8] {
    let h = Sha256::digest(format!("account:{name}").as_bytes());
    let mut d = [0u8; 8];
    d.copy_from_slice(&h[..8]);
    d
}

fn data(name: &str, args: &impl BorshSerialize) -> Vec<u8> {
    let mut v = discriminator(name).to_vec();
    args.serialize(&mut v).expect("borsh serialize");
    v
}

/// Unwind reasons, contract enum `UnwindReason`.
pub const REASON_RULE: u8 = 0;
#[allow(dead_code)]
pub const REASON_EXIT_DEMAND: u8 = 1;
pub const REASON_EMERGENCY: u8 = 2;

#[derive(Clone, Copy, Debug)]
pub struct Pdas {
    pub program_id: Pubkey,
}

impl Pdas {
    pub fn new(program_id: Pubkey) -> Self {
        Self { program_id }
    }
    fn find(&self, seeds: &[&[u8]]) -> Pubkey {
        Pubkey::find_program_address(seeds, &self.program_id).0
    }
    pub fn registry(&self) -> Pubkey {
        self.find(&[b"registry"])
    }
    pub fn vault(&self, xstock_mint: &Pubkey) -> Pubkey {
        self.find(&[b"vault", xstock_mint.as_ref()])
    }
    pub fn share_mint(&self, vault: &Pubkey) -> Pubkey {
        self.find(&[b"shares", vault.as_ref()])
    }
    pub fn stock_custody(&self, vault: &Pubkey) -> Pubkey {
        self.find(&[b"stock", vault.as_ref()])
    }
    pub fn usdc_buffer(&self, vault: &Pubkey) -> Pubkey {
        self.find(&[b"usdc", vault.as_ref()])
    }
    pub fn redeem_stock(&self, vault: &Pubkey) -> Pubkey {
        self.find(&[b"redeem_stock", vault.as_ref()])
    }
    pub fn redeem_usdc(&self, vault: &Pubkey) -> Pubkey {
        self.find(&[b"redeem_usdc", vault.as_ref()])
    }
    pub fn escrow_shares(&self, vault: &Pubkey) -> Pubkey {
        self.find(&[b"escrow", vault.as_ref()])
    }
    pub fn exit_request(&self, vault: &Pubkey, user: &Pubkey, nonce: u64) -> Pubkey {
        self.find(&[b"exit", vault.as_ref(), user.as_ref(), &nonce.to_le_bytes()])
    }
    pub fn exit_epoch(&self, vault: &Pubkey, id: u64) -> Pubkey {
        self.find(&[b"epoch", vault.as_ref(), &id.to_le_bytes()])
    }
}

/// Builds every crank the keeper sends. `keeper` is the signing hot key.
pub struct IxBuilder {
    pub pdas: Pdas,
    pub keeper: Pubkey,
}

impl IxBuilder {
    pub fn new(program_id: Pubkey, keeper: Pubkey) -> Self {
        Self { pdas: Pdas::new(program_id), keeper }
    }

    fn ix(&self, name: &str, args: &impl BorshSerialize, metas: Vec<AccountMeta>) -> Instruction {
        Instruction { program_id: self.pdas.program_id, accounts: metas, data: data(name, args) }
    }

    /// `[keeper (signer), registry, vault]`
    fn keeper_vault_ix(&self, name: &str, vault: &Pubkey, args: &impl BorshSerialize) -> Instruction {
        self.ix(
            name,
            args,
            vec![
                AccountMeta::new(self.keeper, true),
                AccountMeta::new(self.pdas.registry(), false),
                AccountMeta::new(*vault, false),
            ],
        )
    }

    pub fn set_market_open(&self, vault: &Pubkey, open: bool) -> Instruction {
        self.keeper_vault_ix("set_market_open", vault, &open)
    }

    pub fn record_funding(&self, vault: &Pubkey, hawkeye_view: &Pubkey, mock: Option<i64>) -> Instruction {
        self.ix(
            "record_funding",
            &mock,
            vec![
                AccountMeta::new(self.keeper, true),
                AccountMeta::new_readonly(self.pdas.registry(), false),
                AccountMeta::new(*vault, false),
                AccountMeta::new_readonly(*hawkeye_view, false),
            ],
        )
    }

    pub fn record_kamino_rates(&self, kamino_reserve: &Pubkey, mock_borrow: Option<u32>, mock_supply: Option<u32>) -> Instruction {
        self.ix(
            "record_kamino_rates",
            &(mock_borrow, mock_supply),
            vec![
                AccountMeta::new(self.keeper, true),
                AccountMeta::new(self.pdas.registry(), false),
                AccountMeta::new_readonly(*kamino_reserve, false),
            ],
        )
    }

    pub fn refresh_nav(&self, vault: &Pubkey, oracle: &Pubkey, mock_price_e6: Option<u64>) -> Instruction {
        self.ix(
            "refresh_nav",
            &mock_price_e6,
            vec![
                AccountMeta::new(self.keeper, true),
                AccountMeta::new_readonly(self.pdas.registry(), false),
                AccountMeta::new(*vault, false),
                AccountMeta::new_readonly(*oracle, false),
            ],
        )
    }

    pub fn park(&self, vault: &Pubkey) -> Instruction {
        self.keeper_vault_ix("park", vault, &())
    }
    pub fn repay(&self, vault: &Pubkey) -> Instruction {
        self.keeper_vault_ix("repay", vault, &())
    }
    pub fn wind_start(&self, vault: &Pubkey) -> Instruction {
        self.keeper_vault_ix("wind_start", vault, &())
    }
    pub fn wind_step(&self, vault: &Pubkey, n: u8) -> Instruction {
        self.keeper_vault_ix("wind_step", vault, &n)
    }
    pub fn wind_commit(&self, vault: &Pubkey) -> Instruction {
        self.keeper_vault_ix("wind_commit", vault, &())
    }
    pub fn wind_abort(&self, vault: &Pubkey) -> Instruction {
        self.keeper_vault_ix("wind_abort", vault, &())
    }
    pub fn unwind_start(&self, vault: &Pubkey, reason: u8) -> Instruction {
        self.keeper_vault_ix("unwind_start", vault, &reason)
    }
    pub fn unwind_step(&self, vault: &Pubkey, n: u8) -> Instruction {
        self.keeper_vault_ix("unwind_step", vault, &n)
    }
    pub fn unwind_commit(&self, vault: &Pubkey) -> Instruction {
        self.keeper_vault_ix("unwind_commit", vault, &())
    }
    pub fn unwind_partial(&self, vault: &Pubkey, fraction_bps: u32, reason: u8) -> Instruction {
        self.keeper_vault_ix("unwind_partial", vault, &(fraction_bps, reason))
    }
    pub fn size_up(&self, vault: &Pubkey) -> Instruction {
        self.keeper_vault_ix("size_up", vault, &())
    }
    pub fn rebalance_to_kamino(&self, vault: &Pubkey) -> Instruction {
        self.keeper_vault_ix("rebalance_to_kamino", vault, &())
    }
    pub fn rebalance_to_phoenix(&self, vault: &Pubkey) -> Instruction {
        self.keeper_vault_ix("rebalance_to_phoenix", vault, &())
    }
    pub fn rebalance_from_parked(&self, vault: &Pubkey, amount: u64) -> Instruction {
        self.keeper_vault_ix("rebalance_from_parked", vault, &amount)
    }

    pub fn close_epoch(&self, vault: &Pubkey, epoch_id: u64) -> Instruction {
        self.ix(
            "close_epoch",
            &(),
            vec![
                AccountMeta::new(self.keeper, true),
                AccountMeta::new_readonly(self.pdas.registry(), false),
                AccountMeta::new(*vault, false),
                AccountMeta::new(self.pdas.exit_epoch(vault, epoch_id), false),
            ],
        )
    }

    pub fn settle_epoch(&self, vault: &Pubkey, epoch_id: u64) -> Instruction {
        let p = &self.pdas;
        self.ix(
            "settle_epoch",
            &(),
            vec![
                AccountMeta::new(self.keeper, true),
                AccountMeta::new_readonly(p.registry(), false),
                AccountMeta::new(*vault, false),
                AccountMeta::new(p.exit_epoch(vault, epoch_id), false),
                AccountMeta::new(p.stock_custody(vault), false),
                AccountMeta::new(p.usdc_buffer(vault), false),
                AccountMeta::new(p.redeem_stock(vault), false),
                AccountMeta::new(p.redeem_usdc(vault), false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
            ],
        )
    }

    pub fn crystallise_fee(&self, vault: &Pubkey, treasury_shares: &Pubkey) -> Instruction {
        self.ix(
            "crystallise_fee",
            &(),
            vec![
                AccountMeta::new(self.keeper, true),
                AccountMeta::new_readonly(self.pdas.registry(), false),
                AccountMeta::new(*vault, false),
                AccountMeta::new(self.pdas.share_mint(vault), false),
                AccountMeta::new(*treasury_shares, false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
            ],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_discriminator_matches_known_vector() {
        // sha256("global:initialize")[..8] is the well-known Anchor value.
        assert_eq!(discriminator("initialize"), [175, 175, 109, 31, 13, 152, 155, 237]);
    }

    #[test]
    fn instruction_data_is_discriminator_then_borsh_args() {
        let b = IxBuilder::new(Pubkey::new_unique(), Pubkey::new_unique());
        let v = Pubkey::new_unique();
        let ix = b.wind_step(&v, 3);
        assert_eq!(&ix.data[..8], &discriminator("wind_step"));
        assert_eq!(&ix.data[8..], &[3u8]);

        let ix = b.record_funding(&v, &Pubkey::default(), Some(-5));
        assert_eq!(&ix.data[8..], &[1u8, 251, 255, 255, 255, 255, 255, 255, 255]);
        let ix = b.record_funding(&v, &Pubkey::default(), None);
        assert_eq!(&ix.data[8..], &[0u8]);

        let ix = b.unwind_partial(&v, 2500, REASON_EXIT_DEMAND);
        assert_eq!(&ix.data[8..], &[196, 9, 0, 0, 1]);
    }

    #[test]
    fn pdas_use_contract_seeds() {
        let pid = Pubkey::new_unique();
        let p = Pdas::new(pid);
        assert_eq!(p.registry(), Pubkey::find_program_address(&[b"registry"], &pid).0);
        let mint = Pubkey::new_unique();
        let vault = p.vault(&mint);
        assert_eq!(vault, Pubkey::find_program_address(&[b"vault", mint.as_ref()], &pid).0);
        let user = Pubkey::new_unique();
        let exit = p.exit_request(&vault, &user, 7);
        let expected = Pubkey::find_program_address(
            &[b"exit", vault.as_ref(), user.as_ref(), &[7, 0, 0, 0, 0, 0, 0, 0]],
            &pid,
        )
        .0;
        assert_eq!(exit, expected);
        let epoch = p.exit_epoch(&vault, 258);
        let expected = Pubkey::find_program_address(
            &[b"epoch", vault.as_ref(), &[2, 1, 0, 0, 0, 0, 0, 0]],
            &pid,
        )
        .0;
        assert_eq!(epoch, expected);
    }

    #[test]
    fn keeper_signs_first() {
        let k = Pubkey::new_unique();
        let b = IxBuilder::new(Pubkey::new_unique(), k);
        let ix = b.park(&Pubkey::new_unique());
        assert_eq!(ix.accounts[0].pubkey, k);
        assert!(ix.accounts[0].is_signer);
        assert_eq!(ix.accounts[1].pubkey, b.pdas.registry());
        assert_eq!(ix.accounts.len(), 3);
    }
}
