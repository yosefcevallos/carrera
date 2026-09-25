//! Jupiter v6 `shared_accounts_route` CPI (spec §7.4).
//!
//! The keeper fetches the route from Jupiter's swap-instructions API with the
//! vault PDA as `userPublicKey`, substitutes the vault's PDA token accounts for
//! the user source/destination ATAs, and passes the instruction's accounts as the
//! Jupiter block and its data as `VenueData::jupiter_data`. On-chain the program:
//!
//! * checks the program id, the transfer authority (the vault), both mints and
//!   both user token accounts;
//! * patches `in_amount` to the amount it actually wants to swap and
//!   `quoted_out_amount` to its own oracle-derived floor with `slippage_bps = 0`,
//!   so Jupiter itself enforces the floor;
//! * measures the destination balance delta and returns it as the fill.
//!
//! # Jupiter block (`VenueData::blocks & BLOCK_JUPITER`, always last)
//! ```text
//!  0      jupiter_program   JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4
//!  1..N   the swap instruction's accounts in API order
//! ```
//! `shared_accounts_route` data layout: `disc[8] id[1] route_plan(vec) in_amount[u64]
//! quoted_out_amount[u64] slippage_bps[u16] platform_fee_bps[u8]`; the last 19 bytes
//! are the three amounts, which is what gets patched.

use super::{token_amount, VenueCtx};
use crate::errors::CarreraError;
use crate::nav;
use crate::state::OverlayVault;
use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::{AccountMeta, Instruction};

pub const JUPITER_PROGRAM_ID: Pubkey = pubkey!("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4");
pub const D_SHARED_ACCOUNTS_ROUTE: [u8; 8] = [193, 32, 155, 51, 65, 214, 156, 129];

/// Index of the user-facing accounts in `shared_accounts_route`.
const IX_AUTHORITY: usize = 2;
const IX_USER_SOURCE: usize = 3;
const IX_USER_DEST: usize = 6;
const IX_SOURCE_MINT: usize = 7;
const IX_DEST_MINT: usize = 8;
const TAIL: usize = 8 + 8 + 2 + 1;

/// Patch the trailing amounts of a `shared_accounts_route` payload.
pub fn patch_amounts(data: &mut [u8], in_amount: u64, min_out: u64) -> Result<()> {
    require!(data.len() > 8 + 1 + 4 + TAIL && data[..8] == D_SHARED_ACCOUNTS_ROUTE, CarreraError::InvalidArgument);
    let n = data.len();
    data[n - TAIL..n - TAIL + 8].copy_from_slice(&in_amount.to_le_bytes());
    data[n - 11..n - 3].copy_from_slice(&min_out.to_le_bytes());
    data[n - 3..n - 1].copy_from_slice(&0u16.to_le_bytes());
    Ok(())
}

fn swap(ctx: &VenueCtx, source_mint: Pubkey, dest_mint: Pubkey, amount_in: u64, min_out: u64) -> Result<u64> {
    let block = ctx.jupiter()?;
    let program = &block[0];
    require_keys_eq!(*program.key, JUPITER_PROGRAM_ID, CarreraError::VenueAccountsMismatch);
    let route = &block[1..];
    require!(route.len() > IX_DEST_MINT, CarreraError::VenueAccountsMissing);
    require_keys_eq!(*route[IX_AUTHORITY].key, *ctx.vault.key, CarreraError::VenueAccountsMismatch);
    require_keys_eq!(*route[IX_SOURCE_MINT].key, source_mint, CarreraError::VenueAccountsMismatch);
    require_keys_eq!(*route[IX_DEST_MINT].key, dest_mint, CarreraError::VenueAccountsMismatch);
    let (custody, _) = Pubkey::find_program_address(&[b"stock", ctx.vault.key.as_ref()], &crate::ID);
    let (buffer, _) = Pubkey::find_program_address(&[b"usdc", ctx.vault.key.as_ref()], &crate::ID);
    let (want_src, want_dst) = if dest_mint == ctx.xstock_mint { (buffer, custody) } else { (custody, buffer) };
    require_keys_eq!(*route[IX_USER_SOURCE].key, want_src, CarreraError::VenueAccountsMismatch);
    require_keys_eq!(*route[IX_USER_DEST].key, want_dst, CarreraError::VenueAccountsMismatch);

    let mut data = ctx.data.jupiter_data.clone();
    patch_amounts(&mut data, amount_in, min_out)?;

    let accounts: Vec<AccountMeta> = route
        .iter()
        .enumerate()
        .map(|(i, a)| AccountMeta { pubkey: *a.key, is_signer: i == IX_AUTHORITY, is_writable: a.is_writable })
        .collect();
    let before = token_amount(&route[IX_USER_DEST])?;
    ctx.invoke(Instruction { program_id: JUPITER_PROGRAM_ID, accounts, data }, block)?;
    let after = token_amount(&route[IX_USER_DEST])?;
    let out = after.checked_sub(before).ok_or(CarreraError::MathOverflow)?;
    require!(out >= min_out, CarreraError::SlippageExceeded);
    Ok(out)
}

/// Swap `usdc_in` for the vault's xStock. Returns stock base units received.
pub fn swap_usdc_to_stock(ctx: &VenueCtx, vault: &OverlayVault, usdc_in: u64) -> Result<u64> {
    let expected = nav::stock_qty_from_usdc(usdc_in, vault.price_e6, vault.stock_decimals)
        .ok_or_else(|| error!(CarreraError::MathOverflow))?;
    if super::MOCK {
        return Ok(expected);
    }
    let min_out = nav::mul_bps(expected, 10_000 - vault.params.max_swap_slippage_bps).ok_or(CarreraError::MathOverflow)?;
    let usdc_mint = usdc_mint_of(ctx)?;
    swap(ctx, usdc_mint, ctx.xstock_mint, usdc_in, min_out)
}

/// Swap `qty` xStock for USDC. Returns USDC base units received.
pub fn swap_stock_to_usdc(ctx: &VenueCtx, vault: &OverlayVault, qty: u64) -> Result<u64> {
    let expected = nav::stock_value_usdc(qty, vault.price_e6, vault.stock_decimals)
        .ok_or_else(|| error!(CarreraError::MathOverflow))?;
    if super::MOCK {
        return Ok(expected);
    }
    let min_out = nav::mul_bps(expected, 10_000 - vault.params.max_swap_slippage_bps).ok_or(CarreraError::MathOverflow)?;
    let usdc_mint = usdc_mint_of(ctx)?;
    swap(ctx, ctx.xstock_mint, usdc_mint, qty, min_out)
}

/// The USDC mint is read from the Kamino block (index 14) when present, else from the route's mints.
fn usdc_mint_of(ctx: &VenueCtx) -> Result<Pubkey> {
    if let Ok(k) = ctx.kamino() {
        return Ok(*k[14].key);
    }
    let route = &ctx.jupiter()?[1..];
    require!(route.len() > IX_DEST_MINT, CarreraError::VenueAccountsMissing);
    let (a, b) = (*route[IX_SOURCE_MINT].key, *route[IX_DEST_MINT].key);
    Ok(if a == ctx.xstock_mint { b } else { a })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patches_only_the_tail() {
        // disc + id + route_plan(len=1, step: swap(1 byte tag)+percent+in_idx+out_idx) + amounts
        let mut d = D_SHARED_ACCOUNTS_ROUTE.to_vec();
        d.push(0); // id
        d.extend_from_slice(&1u32.to_le_bytes());
        d.extend_from_slice(&[0x11, 100, 0, 1]); // fake step
        d.extend_from_slice(&3_000_000u64.to_le_bytes());
        d.extend_from_slice(&793_680u64.to_le_bytes());
        d.extend_from_slice(&50u16.to_le_bytes());
        d.push(0);
        let head = d[..17].to_vec();
        patch_amounts(&mut d, 2_500_000, 700_000).unwrap();
        assert_eq!(&d[..17], &head[..]);
        let n = d.len();
        assert_eq!(u64::from_le_bytes(d[n - 19..n - 11].try_into().unwrap()), 2_500_000);
        assert_eq!(u64::from_le_bytes(d[n - 11..n - 3].try_into().unwrap()), 700_000);
        assert_eq!(u16::from_le_bytes(d[n - 3..n - 1].try_into().unwrap()), 0);
        assert_eq!(d[n - 1], 0);
    }

    #[test]
    fn rejects_foreign_payloads() {
        let mut d = vec![0u8; 40];
        assert!(patch_amounts(&mut d, 1, 1).is_err());
    }
}
