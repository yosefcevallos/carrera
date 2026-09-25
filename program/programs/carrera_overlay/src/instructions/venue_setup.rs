//! One-time venue setup cranks (keeper-signed, keeper pays rent).

use crate::state::{OverlayVault, Registry};
use crate::venues::{kamino, VenueCtx};
use anchor_lang::prelude::*;

use super::require_keeper;

#[derive(Accounts)]
pub struct InitKaminoObligation<'info> {
    #[account(mut)]
    pub keeper: Signer<'info>,
    #[account(seeds = [b"registry"], bump = registry.bump)]
    pub registry: Account<'info, Registry>,
    /// Writable only because klend wants the obligation owner writable in the CPI; nothing is stored.
    #[account(mut, seeds = [b"vault", vault.xstock_mint.as_ref()], bump = vault.bump)]
    pub vault: Account<'info, OverlayVault>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

/// Create the vault's Kamino user metadata and obligation (tag 0, id 0).
/// Remaining accounts: the Kamino block, then the vault's `user_metadata` PDA.
/// Idempotent: existing accounts are skipped. No-op in mock builds.
pub fn init_kamino_obligation<'info>(
    ctx: Context<'_, '_, '_, 'info, InitKaminoObligation<'info>>,
    venue_data: Vec<u8>,
) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let v = &ctx.accounts.vault;
    let vc = VenueCtx::new(ctx.remaining_accounts, v.to_account_info(), v.xstock_mint, v.bump, &venue_data)?;
    kamino::init_obligation(
        &vc,
        &ctx.accounts.keeper.to_account_info(),
        &ctx.accounts.system_program.to_account_info(),
        &ctx.accounts.rent.to_account_info(),
    )
}
