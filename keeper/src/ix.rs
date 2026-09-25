//! Instruction builders and PDA derivation for `carrera_overlay`, per docs/CONTRACT.md.
//! No dependency on the program crate: discriminators are sha256("global:<name>")[..8],
//! args are Borsh in contract order, accounts follow the contract's row order with the
//! signer first.

use crate::venue_accounts::VenueArgs;
use borsh::BorshSerialize;
use sha2::{Digest, Sha256};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey,
    pubkey::Pubkey,
};

pub const TOKEN_PROGRAM_ID: Pubkey = pubkey!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
/// Token-2022; the program used by all nine mainnet xStock mints.
pub const TOKEN_2022_PROGRAM_ID: Pubkey = pubkey!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");

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
        self.refresh_nav_with(vault, oracle, mock_price_e6, Vec::new())
    }

    /// `refresh_nav` with remaining accounts (real build: `[klend, lending_market, scope_prices]`
    /// so the program refreshes the reserve before reading its price).
    pub fn refresh_nav_with(&self, vault: &Pubkey, oracle: &Pubkey, mock_price_e6: Option<u64>, extra: Vec<AccountMeta>) -> Instruction {
        let mut metas = vec![
            AccountMeta::new(self.keeper, true),
            AccountMeta::new_readonly(self.pdas.registry(), false),
            AccountMeta::new(*vault, false),
            AccountMeta::new_readonly(*oracle, false),
        ];
        metas.extend(extra);
        self.ix("refresh_nav", &mock_price_e6, metas)
    }

    /// `[keeper (signer), registry, vault] + venue.remaining`, args + trailing `venue_data`.
    fn engine_ix(&self, name: &str, vault: &Pubkey, args: &impl BorshSerialize, venue: &VenueArgs) -> Instruction {
        let mut metas = vec![
            AccountMeta::new(self.keeper, true),
            AccountMeta::new(self.pdas.registry(), false),
            AccountMeta::new(*vault, false),
        ];
        metas.extend(venue.remaining.iter().cloned());
        self.ix(name, &(args, &venue.data), metas)
    }

    pub fn park(&self, vault: &Pubkey, venue: &VenueArgs) -> Instruction {
        self.engine_ix("park", vault, &(), venue)
    }
    pub fn repay(&self, vault: &Pubkey, venue: &VenueArgs) -> Instruction {
        self.engine_ix("repay", vault, &(), venue)
    }
    /// Move custody stock into the Kamino obligation (no-op on mock builds).
    pub fn sync_collateral(&self, vault: &Pubkey, venue: &VenueArgs) -> Instruction {
        self.engine_ix("sync_collateral", vault, &(), venue)
    }
    pub fn wind_start(&self, vault: &Pubkey, venue: &VenueArgs) -> Instruction {
        self.engine_ix("wind_start", vault, &(), venue)
    }
    pub fn wind_step(&self, vault: &Pubkey, n: u8, venue: &VenueArgs) -> Instruction {
        self.engine_ix("wind_step", vault, &n, venue)
    }
    pub fn wind_commit(&self, vault: &Pubkey, venue: &VenueArgs) -> Instruction {
        self.engine_ix("wind_commit", vault, &(), venue)
    }
    pub fn wind_abort(&self, vault: &Pubkey) -> Instruction {
        self.keeper_vault_ix("wind_abort", vault, &())
    }
    pub fn unwind_start(&self, vault: &Pubkey, reason: u8) -> Instruction {
        self.keeper_vault_ix("unwind_start", vault, &reason)
    }
    pub fn unwind_step(&self, vault: &Pubkey, n: u8, venue: &VenueArgs) -> Instruction {
        self.engine_ix("unwind_step", vault, &n, venue)
    }
    pub fn unwind_commit(&self, vault: &Pubkey, venue: &VenueArgs) -> Instruction {
        self.engine_ix("unwind_commit", vault, &(), venue)
    }
    pub fn unwind_partial(&self, vault: &Pubkey, fraction_bps: u32, reason: u8, venue: &VenueArgs) -> Instruction {
        self.engine_ix("unwind_partial", vault, &(fraction_bps, reason), venue)
    }
    pub fn size_up(&self, vault: &Pubkey, venue: &VenueArgs) -> Instruction {
        self.engine_ix("size_up", vault, &(), venue)
    }
    pub fn rebalance_to_kamino(&self, vault: &Pubkey, venue: &VenueArgs) -> Instruction {
        self.engine_ix("rebalance_to_kamino", vault, &(), venue)
    }
    pub fn rebalance_to_phoenix(&self, vault: &Pubkey, venue: &VenueArgs) -> Instruction {
        self.engine_ix("rebalance_to_phoenix", vault, &(), venue)
    }
    pub fn rebalance_from_parked(&self, vault: &Pubkey, amount: u64, venue: &VenueArgs) -> Instruction {
        self.engine_ix("rebalance_from_parked", vault, &amount, venue)
    }
    /// One-time: create the vault's Kamino user metadata + obligation (keeper pays rent).
    /// Remaining accounts: the Kamino block, then the vault's `user_metadata` PDA.
    pub fn init_kamino_obligation(&self, vault: &Pubkey, venue: &VenueArgs) -> Instruction {
        let mut metas = vec![
            AccountMeta::new(self.keeper, true),
            AccountMeta::new_readonly(self.pdas.registry(), false),
            AccountMeta::new(*vault, false),
            AccountMeta::new_readonly(solana_sdk::system_program::id(), false),
            AccountMeta::new_readonly(solana_sdk::sysvar::rent::id(), false),
        ];
        metas.extend(venue.remaining.iter().cloned());
        self.ix("init_kamino_obligation", &venue.data, metas)
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
                AccountMeta::new_readonly(solana_sdk::system_program::id(), false),
            ],
        )
    }

    /// `xstock_mint` and `stock_token_program` (Token-2022 for xStocks) are the two
    /// accounts the program added when it gained Token-2022 stock support.
    pub fn settle_epoch(&self, vault: &Pubkey, epoch_id: u64, xstock_mint: &Pubkey, stock_token_program: &Pubkey, venue: &VenueArgs) -> Instruction {
        let p = &self.pdas;
        let mut metas = vec![
                AccountMeta::new(self.keeper, true),
                AccountMeta::new_readonly(p.registry(), false),
                AccountMeta::new(*vault, false),
                AccountMeta::new(p.exit_epoch(vault, epoch_id), false),
                AccountMeta::new(p.stock_custody(vault), false),
                AccountMeta::new(p.usdc_buffer(vault), false),
                AccountMeta::new(p.redeem_stock(vault), false),
                AccountMeta::new(p.redeem_usdc(vault), false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
                AccountMeta::new_readonly(*xstock_mint, false),
                AccountMeta::new_readonly(*stock_token_program, false),
            ];
        metas.extend(venue.remaining.iter().cloned());
        self.ix("settle_epoch", &venue.data, metas)
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
        let ix = b.wind_step(&v, 3, &VenueArgs::none());
        assert_eq!(&ix.data[..8], &discriminator("wind_step"));
        // n, then the empty `venue_data: Vec<u8>` (Borsh length prefix 0).
        assert_eq!(&ix.data[8..], &[3u8, 0, 0, 0, 0]);
        assert_eq!(ix.accounts.len(), 3, "no venue blocks when venue_data is empty");

        let ix = b.record_funding(&v, &Pubkey::default(), Some(-5));
        assert_eq!(&ix.data[8..], &[1u8, 251, 255, 255, 255, 255, 255, 255, 255]);
        let ix = b.record_funding(&v, &Pubkey::default(), None);
        assert_eq!(&ix.data[8..], &[0u8]);

        let ix = b.unwind_partial(&v, 2500, REASON_EXIT_DEMAND, &VenueArgs::none());
        assert_eq!(&ix.data[8..], &[196, 9, 0, 0, 1, 0, 0, 0, 0]);
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
        let ix = b.park(&Pubkey::new_unique(), &VenueArgs::none());
        assert_eq!(ix.accounts[0].pubkey, k);
        assert!(ix.accounts[0].is_signer);
        assert_eq!(ix.accounts[1].pubkey, b.pdas.registry());
        assert_eq!(ix.accounts.len(), 3);
    }

    #[test]
    fn settle_epoch_carries_token_2022_accounts() {
        let b = IxBuilder::new(Pubkey::new_unique(), Pubkey::new_unique());
        let vault = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let ix = b.settle_epoch(&vault, 7, &mint, &TOKEN_2022_PROGRAM_ID, &VenueArgs::none());
        // keeper, registry, vault, exit_epoch, stock_custody, usdc_buffer, redeem_stock, redeem_usdc,
        // token_program, xstock_mint, stock_token_program
        assert_eq!(ix.accounts.len(), 11);
        assert_eq!(ix.accounts[8].pubkey, TOKEN_PROGRAM_ID);
        assert_eq!(ix.accounts[9].pubkey, mint);
        assert!(!ix.accounts[9].is_writable);
        assert_eq!(ix.accounts[10].pubkey, TOKEN_2022_PROGRAM_ID);
        assert_eq!(ix.accounts[3].pubkey, b.pdas.exit_epoch(&vault, 7));
    }
}
