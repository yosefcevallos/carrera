use crate::errors::CarreraError;
use crate::events::{Deposited, ExitCancelled, ExitRequested, Redeemed};
use crate::state::{ExitEpoch, ExitRequest, ExitStatus, OverlayVault, Registry};
use crate::venues;
use anchor_lang::prelude::*;
use anchor_spl::token::{self, Burn, Mint, MintTo, Token, TokenAccount, Transfer};

use super::{depositor_qty, recompute_nav, require_nav_fresh, require_not_paused, vault_key};

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(seeds = [b"registry"], bump = registry.bump)]
    pub registry: Box<Account<'info, Registry>>,
    #[account(mut, seeds = [b"vault", vault.xstock_mint.as_ref()], bump = vault.bump, has_one = share_mint, has_one = xstock_mint)]
    pub vault: Box<Account<'info, OverlayVault>>,
    #[account(mut)]
    pub share_mint: Box<Account<'info, Mint>>,
    pub xstock_mint: Box<Account<'info, Mint>>,
    #[account(mut, seeds = [b"stock", vault.key().as_ref()], bump)]
    pub stock_custody: Box<Account<'info, TokenAccount>>,
    #[account(mut, token::mint = xstock_mint, token::authority = user)]
    pub user_stock: Box<Account<'info, TokenAccount>>,
    #[account(mut, token::mint = share_mint, token::authority = user)]
    pub user_shares: Box<Account<'info, TokenAccount>>,
    pub token_program: Program<'info, Token>,
}

pub fn deposit(ctx: Context<Deposit>, qty: u64, min_shares: u64) -> Result<()> {
    require_not_paused(&ctx.accounts.registry)?;
    require!(qty > 0, CarreraError::InvalidArgument);
    let v = &mut ctx.accounts.vault;
    require!(v.state != crate::state::VaultState::Unwinding as u8, CarreraError::WrongState);
    require_nav_fresh(v)?;
    if v.params.deposit_cap_stock > 0 {
        require!(
            depositor_qty(v).saturating_add(qty) <= v.params.deposit_cap_stock,
            CarreraError::DepositCapExceeded
        );
    }
    let shares: u64 = if v.total_shares == 0 {
        qty
    } else {
        require!(v.share_price_stock_e6 > 0, CarreraError::MathOverflow);
        ((qty as u128) * 1_000_000 / (v.share_price_stock_e6 as u128))
            .try_into()
            .map_err(|_| CarreraError::MathOverflow)?
    };
    require!(shares >= min_shares && shares > 0, CarreraError::SlippageExceeded);

    token::transfer(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.user_stock.to_account_info(),
                to: ctx.accounts.stock_custody.to_account_info(),
                authority: ctx.accounts.user.to_account_info(),
            },
        ),
        qty,
    )?;
    venues::kamino::deposit_collateral(qty)?;

    let xstock_mint = v.xstock_mint;
    let bump = v.bump;
    let seeds: &[&[u8]] = &[b"vault", xstock_mint.as_ref(), &[bump]];
    token::mint_to(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            MintTo {
                mint: ctx.accounts.share_mint.to_account_info(),
                to: ctx.accounts.user_shares.to_account_info(),
                authority: v.to_account_info(),
            },
            &[seeds],
        ),
        shares,
    )?;

    v.collateral_qty = v.collateral_qty.checked_add(qty).ok_or(CarreraError::MathOverflow)?;
    v.total_shares = v.total_shares.checked_add(shares).ok_or(CarreraError::MathOverflow)?;
    recompute_nav(v)?;
    emit!(Deposited { vault: vault_key(v), user: ctx.accounts.user.key(), qty, shares });
    Ok(())
}

#[derive(Accounts)]
#[instruction(shares: u64, nonce: u64)]
pub struct RequestExit<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(seeds = [b"registry"], bump = registry.bump)]
    pub registry: Box<Account<'info, Registry>>,
    #[account(mut, seeds = [b"vault", vault.xstock_mint.as_ref()], bump = vault.bump, has_one = share_mint)]
    pub vault: Box<Account<'info, OverlayVault>>,
    #[account(init, payer = user, space = 8 + ExitRequest::INIT_SPACE,
        seeds = [b"exit", vault.key().as_ref(), user.key().as_ref(), &nonce.to_le_bytes()], bump)]
    pub exit_request: Box<Account<'info, ExitRequest>>,
    #[account(init_if_needed, payer = user, space = 8 + ExitEpoch::INIT_SPACE,
        seeds = [b"epoch", vault.key().as_ref(), &vault.epoch_id.to_le_bytes()], bump)]
    pub exit_epoch: Box<Account<'info, ExitEpoch>>,
    pub share_mint: Box<Account<'info, Mint>>,
    #[account(mut, token::mint = share_mint, token::authority = user)]
    pub user_shares: Box<Account<'info, TokenAccount>>,
    #[account(init_if_needed, payer = user, seeds = [b"escrow", vault.key().as_ref()], bump,
        token::mint = share_mint, token::authority = vault)]
    pub escrow_shares: Box<Account<'info, TokenAccount>>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

pub fn request_exit(ctx: Context<RequestExit>, shares: u64, nonce: u64) -> Result<()> {
    require!(shares > 0, CarreraError::InvalidArgument);
    let v = &mut ctx.accounts.vault;
    let e = &mut ctx.accounts.exit_epoch;
    if e.vault == Pubkey::default() {
        e.vault = v.key();
        e.id = v.epoch_id;
        e.bump = ctx.bumps.exit_epoch;
    }
    require!(!e.closed, CarreraError::WrongState);

    token::transfer(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.user_shares.to_account_info(),
                to: ctx.accounts.escrow_shares.to_account_info(),
                authority: ctx.accounts.user.to_account_info(),
            },
        ),
        shares,
    )?;

    let r = &mut ctx.accounts.exit_request;
    r.vault = v.key();
    r.user = ctx.accounts.user.key();
    r.nonce = nonce;
    r.shares = shares;
    r.epoch_id = v.epoch_id;
    r.status = ExitStatus::Open as u8;
    r.bump = ctx.bumps.exit_request;

    e.shares_total = e.shares_total.checked_add(shares).ok_or(CarreraError::MathOverflow)?;
    v.pending_exit_shares = v.pending_exit_shares.checked_add(shares).ok_or(CarreraError::MathOverflow)?;
    emit!(ExitRequested { vault: v.key(), user: r.user, shares, epoch_id: r.epoch_id });
    Ok(())
}

#[derive(Accounts)]
pub struct CancelExit<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(mut, seeds = [b"vault", vault.xstock_mint.as_ref()], bump = vault.bump, has_one = share_mint)]
    pub vault: Box<Account<'info, OverlayVault>>,
    #[account(mut, close = user, has_one = vault, has_one = user,
        seeds = [b"exit", vault.key().as_ref(), user.key().as_ref(), &exit_request.nonce.to_le_bytes()], bump = exit_request.bump)]
    pub exit_request: Box<Account<'info, ExitRequest>>,
    #[account(mut, has_one = vault, seeds = [b"epoch", vault.key().as_ref(), &exit_request.epoch_id.to_le_bytes()], bump = exit_epoch.bump)]
    pub exit_epoch: Box<Account<'info, ExitEpoch>>,
    pub share_mint: Box<Account<'info, Mint>>,
    #[account(mut, token::mint = share_mint, token::authority = user)]
    pub user_shares: Box<Account<'info, TokenAccount>>,
    #[account(mut, seeds = [b"escrow", vault.key().as_ref()], bump)]
    pub escrow_shares: Box<Account<'info, TokenAccount>>,
    pub token_program: Program<'info, Token>,
}

pub fn cancel_exit(ctx: Context<CancelExit>) -> Result<()> {
    let r = &ctx.accounts.exit_request;
    require!(r.status == ExitStatus::Open as u8, CarreraError::WrongState);
    require!(!ctx.accounts.exit_epoch.closed, CarreraError::WrongState);
    let shares = r.shares;
    let v = &mut ctx.accounts.vault;
    let seeds: &[&[u8]] = &[b"vault", v.xstock_mint.as_ref(), &[v.bump]];
    token::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.escrow_shares.to_account_info(),
                to: ctx.accounts.user_shares.to_account_info(),
                authority: v.to_account_info(),
            },
            &[seeds],
        ),
        shares,
    )?;
    let e = &mut ctx.accounts.exit_epoch;
    e.shares_total = e.shares_total.checked_sub(shares).ok_or(CarreraError::MathOverflow)?;
    v.pending_exit_shares = v.pending_exit_shares.checked_sub(shares).ok_or(CarreraError::MathOverflow)?;
    emit!(ExitCancelled { vault: v.key(), user: ctx.accounts.user.key(), shares });
    Ok(())
}

#[derive(Accounts)]
pub struct Redeem<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(seeds = [b"vault", vault.xstock_mint.as_ref()], bump = vault.bump, has_one = share_mint)]
    pub vault: Box<Account<'info, OverlayVault>>,
    #[account(mut, close = user, has_one = vault, has_one = user,
        seeds = [b"exit", vault.key().as_ref(), user.key().as_ref(), &exit_request.nonce.to_le_bytes()], bump = exit_request.bump)]
    pub exit_request: Box<Account<'info, ExitRequest>>,
    #[account(has_one = vault, seeds = [b"epoch", vault.key().as_ref(), &exit_request.epoch_id.to_le_bytes()], bump = exit_epoch.bump)]
    pub exit_epoch: Box<Account<'info, ExitEpoch>>,
    #[account(mut)]
    pub share_mint: Box<Account<'info, Mint>>,
    #[account(mut, seeds = [b"escrow", vault.key().as_ref()], bump)]
    pub escrow_shares: Box<Account<'info, TokenAccount>>,
    #[account(mut, seeds = [b"redeem_stock", vault.key().as_ref()], bump)]
    pub redeem_stock: Box<Account<'info, TokenAccount>>,
    #[account(mut, seeds = [b"redeem_usdc", vault.key().as_ref()], bump)]
    pub redeem_usdc: Box<Account<'info, TokenAccount>>,
    #[account(mut, token::authority = user)]
    pub user_stock: Box<Account<'info, TokenAccount>>,
    #[account(mut, token::authority = user)]
    pub user_usdc: Box<Account<'info, TokenAccount>>,
    pub token_program: Program<'info, Token>,
}

pub fn redeem(ctx: Context<Redeem>) -> Result<()> {
    let e = &ctx.accounts.exit_epoch;
    require!(e.settled, CarreraError::EpochNotSettled);
    let r = &ctx.accounts.exit_request;
    require!(r.status == ExitStatus::Open as u8, CarreraError::WrongState);
    let stock: u64 = ((r.shares as u128) * (e.stock_per_share_e6 as u128) / 1_000_000)
        .try_into()
        .map_err(|_| CarreraError::MathOverflow)?;
    let usdc: u64 = ((r.shares as u128) * (e.usdc_per_share_e6 as u128) / 1_000_000)
        .try_into()
        .map_err(|_| CarreraError::MathOverflow)?;
    let v = &ctx.accounts.vault;
    let seeds: &[&[u8]] = &[b"vault", v.xstock_mint.as_ref(), &[v.bump]];
    token::burn(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Burn {
                mint: ctx.accounts.share_mint.to_account_info(),
                from: ctx.accounts.escrow_shares.to_account_info(),
                authority: v.to_account_info(),
            },
            &[seeds],
        ),
        r.shares,
    )?;
    if stock > 0 {
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.redeem_stock.to_account_info(),
                    to: ctx.accounts.user_stock.to_account_info(),
                    authority: v.to_account_info(),
                },
                &[seeds],
            ),
            stock,
        )?;
    }
    if usdc > 0 {
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.redeem_usdc.to_account_info(),
                    to: ctx.accounts.user_usdc.to_account_info(),
                    authority: v.to_account_info(),
                },
                &[seeds],
            ),
            usdc,
        )?;
    }
    emit!(Redeemed { vault: v.key(), user: ctx.accounts.user.key(), shares: r.shares, stock, usdc });
    Ok(())
}
