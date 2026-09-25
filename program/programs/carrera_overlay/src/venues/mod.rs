//! Venue adapters. Every external leg the program executes goes through one of
//! these functions. With `mock-venues` they simulate the leg from the vault's
//! cached oracle price and keeper-supplied values; without it they CPI into
//! Kamino Lend, Jupiter and Phoenix (spec §7, M2/M3).
//!
//! Accounts for the CPIs arrive in `remaining_accounts` as up to three fixed-order
//! blocks (Kamino, Phoenix, Jupiter; see `VenueData::blocks` and the per-module
//! docs) and per-call parameters arrive as a Borsh `VenueData` in the
//! instruction's trailing `venue_data: Vec<u8>` argument. Mock builds ignore both.

pub mod hawkeye;
pub mod jupiter;
pub mod kamino;
pub mod phoenix;

use crate::errors::CarreraError;
use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::{AccountMeta, Instruction};
use anchor_lang::solana_program::program::invoke_signed;

#[cfg(feature = "mock-venues")]
pub const MOCK: bool = true;
#[cfg(not(feature = "mock-venues"))]
pub const MOCK: bool = false;

pub const BLOCK_KAMINO: u8 = 1;
pub const BLOCK_PHOENIX: u8 = 2;
pub const BLOCK_JUPITER: u8 = 4;

/// Per-call venue parameters supplied by the keeper (Borsh, trailing `venue_data` arg).
/// An empty `venue_data` decodes to the default (no blocks), which is what mock builds use.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default, Debug, PartialEq, Eq)]
pub struct VenueData {
    /// Which account blocks follow in `remaining_accounts`, always in the order
    /// Kamino (24 accounts), Phoenix (16 + gti + atb), Jupiter (the rest).
    pub blocks: u8,
    /// Phoenix: number of global-trader-index accounts, then active-trader-buffer accounts.
    pub phoenix_gti: u8,
    pub phoenix_atb: u8,
    /// Phoenix market: stock base units per base lot (keeper-read, DECISIONS D6).
    pub base_lot_size: u64,
    /// Phoenix IOC limit price in ticks (0 = no limit) and expiry slot (0 = none).
    pub price_in_ticks: u64,
    pub last_valid_slot: u64,
    /// Phoenix subaccount equity in USDC base units, keeper-read (DECISIONS D6).
    pub phoenix_equity_usdc: u64,
    pub client_order_id: u64,
    /// Jupiter `shared_accounts_route` data exactly as the swap-instructions API
    /// returned it; `in_amount`, `quoted_out_amount` and `slippage_bps` are patched on-chain.
    pub jupiter_data: Vec<u8>,
}

/// Fixed number of accounts in the Kamino block.
pub const KAMINO_BLOCK_LEN: usize = 24;
/// Fixed prefix of the Phoenix block (before the trader-index accounts).
pub const PHOENIX_BLOCK_FIXED: usize = 16;

/// Everything an adapter needs to execute a venue leg on behalf of the vault PDA.
pub struct VenueCtx<'a, 'info> {
    pub remaining: &'a [AccountInfo<'info>],
    pub vault: AccountInfo<'info>,
    pub xstock_mint: Pubkey,
    bump: [u8; 1],
    pub data: VenueData,
}

impl<'a, 'info> VenueCtx<'a, 'info> {
    pub fn new(
        remaining: &'a [AccountInfo<'info>],
        vault: AccountInfo<'info>,
        xstock_mint: Pubkey,
        bump: u8,
        raw: &[u8],
    ) -> Result<Self> {
        let data = if raw.is_empty() {
            VenueData::default()
        } else {
            VenueData::try_from_slice(raw).map_err(|_| error!(CarreraError::InvalidArgument))?
        };
        Ok(Self { remaining, vault, xstock_mint, bump: [bump], data })
    }

    /// A context with default venue data but the raw remaining accounts (used by
    /// `refresh_nav`, whose remaining accounts are not block-structured).
    pub fn empty_with(remaining: &'a [AccountInfo<'info>], vault: AccountInfo<'info>, xstock_mint: Pubkey, bump: u8) -> Self {
        Self { remaining, vault, xstock_mint, bump: [bump], data: VenueData::default() }
    }

    pub fn seeds(&self) -> [&[u8]; 3] {
        [b"vault", self.xstock_mint.as_ref(), &self.bump]
    }

    fn block_start(&self, block: u8) -> Result<usize> {
        let mut off = 0usize;
        if self.data.blocks & BLOCK_KAMINO != 0 {
            if block == BLOCK_KAMINO {
                return Ok(off);
            }
            off += KAMINO_BLOCK_LEN;
        }
        if self.data.blocks & BLOCK_PHOENIX != 0 {
            if block == BLOCK_PHOENIX {
                return Ok(off);
            }
            off += PHOENIX_BLOCK_FIXED + self.data.phoenix_gti as usize + self.data.phoenix_atb as usize;
        }
        if self.data.blocks & BLOCK_JUPITER != 0 && block == BLOCK_JUPITER {
            return Ok(off);
        }
        err!(CarreraError::VenueAccountsMissing)
    }

    pub fn kamino(&self) -> Result<&'a [AccountInfo<'info>]> {
        let s = self.block_start(BLOCK_KAMINO)?;
        self.remaining.get(s..s + KAMINO_BLOCK_LEN).ok_or_else(|| error!(CarreraError::VenueAccountsMissing))
    }

    pub fn phoenix(&self) -> Result<&'a [AccountInfo<'info>]> {
        let s = self.block_start(BLOCK_PHOENIX)?;
        let n = PHOENIX_BLOCK_FIXED + self.data.phoenix_gti as usize + self.data.phoenix_atb as usize;
        self.remaining.get(s..s + n).ok_or_else(|| error!(CarreraError::VenueAccountsMissing))
    }

    pub fn jupiter(&self) -> Result<&'a [AccountInfo<'info>]> {
        let s = self.block_start(BLOCK_JUPITER)?;
        let b = self.remaining.get(s..).ok_or_else(|| error!(CarreraError::VenueAccountsMissing))?;
        require!(b.len() >= 2, CarreraError::VenueAccountsMissing);
        Ok(b)
    }

    /// Invoke `ix` signed by the vault PDA. Every account meta must be present in
    /// `pool` (matched by key); the vault itself is always available.
    pub fn invoke(&self, ix: Instruction, pool: &[AccountInfo<'info>]) -> Result<()> {
        let mut infos: Vec<AccountInfo<'info>> = Vec::with_capacity(ix.accounts.len() + 1);
        for meta in &ix.accounts {
            let ai = if meta.pubkey == *self.vault.key {
                self.vault.clone()
            } else {
                pool.iter()
                    .find(|a| a.key == &meta.pubkey)
                    .cloned()
                    .ok_or_else(|| error!(CarreraError::VenueAccountsMissing))?
            };
            infos.push(ai);
        }
        // The program account itself.
        if let Some(p) = pool.iter().find(|a| a.key == &ix.program_id) {
            infos.push(p.clone());
        }
        let seeds = self.seeds();
        invoke_signed(&ix, &infos, &[&seeds]).map_err(|e| {
            msg!("venue CPI failed: {:?}", e);
            error!(CarreraError::VenueCpiFailed)
        })
    }
}

/// SPL token account balance (`amount` at offset 64), valid for Token and Token-2022.
pub fn token_amount(ai: &AccountInfo) -> Result<u64> {
    let d = ai.try_borrow_data()?;
    require!(d.len() >= 72, CarreraError::InvalidArgument);
    Ok(u64::from_le_bytes(d[64..72].try_into().unwrap()))
}

pub(crate) fn meta(pubkey: Pubkey, writable: bool, signer: bool) -> AccountMeta {
    if writable {
        AccountMeta::new(pubkey, signer)
    } else {
        AccountMeta::new_readonly(pubkey, signer)
    }
}
