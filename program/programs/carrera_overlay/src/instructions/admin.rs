use crate::errors::CarreraError;
use crate::events::{Paused, Unpaused};
use crate::state::{OverlayVault, Registry, VaultParams, VaultState, MAX_KEEPERS};
use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};

use super::require_keeper;

#[derive(Accounts)]
pub struct InitRegistry<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(init, payer = admin, space = 8 + Registry::INIT_SPACE, seeds = [b"registry"], bump)]
    pub registry: Account<'info, Registry>,
    pub usdc_mint: Account<'info, Mint>,
    pub system_program: Program<'info, System>,
}

pub fn init_registry(ctx: Context<InitRegistry>, guardian: Pubkey, keepers: Vec<Pubkey>) -> Result<()> {
    require!(keepers.len() <= MAX_KEEPERS, CarreraError::InvalidArgument);
    let r = &mut ctx.accounts.registry;
    r.admin = ctx.accounts.admin.key();
    r.guardian = guardian;
    r.keepers = [Pubkey::default(); MAX_KEEPERS];
    for (i, k) in keepers.iter().enumerate() {
        r.keepers[i] = *k;
    }
    r.keeper_count = keepers.len() as u8;
    r.paused = false;
    r.usdc_mint = ctx.accounts.usdc_mint.key();
    r.bump = ctx.bumps.registry;
    Ok(())
}

#[derive(Accounts)]
pub struct AdminRegistry<'info> {
    pub admin: Signer<'info>,
    #[account(mut, seeds = [b"registry"], bump = registry.bump, has_one = admin @ CarreraError::Unauthorized)]
    pub registry: Account<'info, Registry>,
}

pub fn set_roles(
    ctx: Context<AdminRegistry>,
    admin: Option<Pubkey>,
    guardian: Option<Pubkey>,
    keepers: Option<Vec<Pubkey>>,
) -> Result<()> {
    let r = &mut ctx.accounts.registry;
    if let Some(a) = admin {
        r.admin = a;
    }
    if let Some(g) = guardian {
        r.guardian = g;
    }
    if let Some(ks) = keepers {
        require!(ks.len() <= MAX_KEEPERS, CarreraError::InvalidArgument);
        r.keepers = [Pubkey::default(); MAX_KEEPERS];
        for (i, k) in ks.iter().enumerate() {
            r.keepers[i] = *k;
        }
        r.keeper_count = ks.len() as u8;
    }
    Ok(())
}

#[derive(Accounts)]
pub struct PauseRegistry<'info> {
    pub signer: Signer<'info>,
    #[account(mut, seeds = [b"registry"], bump = registry.bump)]
    pub registry: Account<'info, Registry>,
}

pub fn pause(ctx: Context<PauseRegistry>) -> Result<()> {
    let r = &mut ctx.accounts.registry;
    let s = ctx.accounts.signer.key();
    require!(r.admin == s || r.guardian == s, CarreraError::Unauthorized);
    r.paused = true;
    emit!(Paused {});
    Ok(())
}

pub fn unpause(ctx: Context<PauseRegistry>) -> Result<()> {
    let r = &mut ctx.accounts.registry;
    require!(r.admin == ctx.accounts.signer.key(), CarreraError::Unauthorized);
    r.paused = false;
    emit!(Unpaused {});
    Ok(())
}

#[derive(Accounts)]
pub struct InitVault<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(seeds = [b"registry"], bump = registry.bump, has_one = admin @ CarreraError::Unauthorized, has_one = usdc_mint)]
    pub registry: Box<Account<'info, Registry>>,
    #[account(init, payer = admin, space = 8 + OverlayVault::INIT_SPACE, seeds = [b"vault", xstock_mint.key().as_ref()], bump)]
    pub vault: Box<Account<'info, OverlayVault>>,
    pub xstock_mint: Box<Account<'info, Mint>>,
    #[account(init, payer = admin, seeds = [b"shares", vault.key().as_ref()], bump,
        mint::decimals = xstock_mint.decimals, mint::authority = vault, mint::freeze_authority = vault)]
    pub share_mint: Box<Account<'info, Mint>>,
    #[account(init, payer = admin, seeds = [b"stock", vault.key().as_ref()], bump,
        token::mint = xstock_mint, token::authority = vault)]
    pub stock_custody: Box<Account<'info, TokenAccount>>,
    #[account(init, payer = admin, seeds = [b"usdc", vault.key().as_ref()], bump,
        token::mint = usdc_mint, token::authority = vault)]
    pub usdc_buffer: Box<Account<'info, TokenAccount>>,
    #[account(init, payer = admin, seeds = [b"redeem_stock", vault.key().as_ref()], bump,
        token::mint = xstock_mint, token::authority = vault)]
    pub redeem_stock: Box<Account<'info, TokenAccount>>,
    #[account(init, payer = admin, seeds = [b"redeem_usdc", vault.key().as_ref()], bump,
        token::mint = usdc_mint, token::authority = vault)]
    pub redeem_usdc: Box<Account<'info, TokenAccount>>,
    pub usdc_mint: Box<Account<'info, Mint>>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

pub fn init_vault(ctx: Context<InitVault>, tier: u8, params: VaultParams) -> Result<()> {
    require!(tier <= 3, CarreraError::InvalidArgument);
    validate_params(&params)?;
    let v = &mut ctx.accounts.vault;
    v.xstock_mint = ctx.accounts.xstock_mint.key();
    v.share_mint = ctx.accounts.share_mint.key();
    v.tier = tier;
    v.params = params;
    v.state = VaultState::Idle as u8;
    v.step = 0;
    v.market_open = false;
    v.share_price_stock_e6 = 1_000_000;
    v.high_water_e6 = 1_000_000;
    v.epoch_id = 0;
    v.epoch_opened_ts = Clock::get()?.unix_timestamp;
    v.stock_decimals = ctx.accounts.xstock_mint.decimals;
    v.bump = ctx.bumps.vault;
    Ok(())
}

fn validate_params(p: &VaultParams) -> Result<()> {
    require!(p.ltv_bps > 0 && p.ltv_bps < 10_000, CarreraError::InvalidArgument);
    require!(p.funding_window as usize <= crate::state::FUNDING_WINDOW && p.funding_window > 0, CarreraError::InvalidArgument);
    require!(p.expected_hold_hours > 0, CarreraError::InvalidArgument);
    require!(p.perf_fee_bps <= 10_000 && p.exit_fee_bps <= 10_000, CarreraError::InvalidArgument);
    require!(p.epoch_len_secs > 0, CarreraError::InvalidArgument);
    Ok(())
}

#[derive(Accounts)]
pub struct AdminVault<'info> {
    pub admin: Signer<'info>,
    #[account(seeds = [b"registry"], bump = registry.bump, has_one = admin @ CarreraError::Unauthorized)]
    pub registry: Account<'info, Registry>,
    #[account(mut, seeds = [b"vault", vault.xstock_mint.as_ref()], bump = vault.bump)]
    pub vault: Account<'info, OverlayVault>,
}

pub fn set_params(ctx: Context<AdminVault>, params: VaultParams) -> Result<()> {
    validate_params(&params)?;
    ctx.accounts.vault.params = params;
    Ok(())
}

pub fn set_market_open(ctx: Context<super::KeeperVault>, open: bool) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    ctx.accounts.vault.market_open = open;
    Ok(())
}
