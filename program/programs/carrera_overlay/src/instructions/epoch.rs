//! Exit epochs (spec §8) and performance fee (spec §9).

use crate::errors::CarreraError;
use crate::events::{EpochClosed, EpochSettled, FeeCrystallised};
use crate::nav;
use crate::state::{ExitEpoch, OverlayVault, Registry, VaultState};
use crate::venues::{kamino, VenueCtx};
use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, MintTo, Token, TokenAccount, Transfer};
use anchor_spl::token_interface::{
    self as token_2022, Mint as StockMint, TokenAccount as StockAccount, TokenInterface, TransferChecked,
};

use super::{depositor_qty, effective_debt, mul_bps, recompute_nav, require_keeper, require_nav_fresh, stock_value};

#[derive(Accounts)]
pub struct CloseEpoch<'info> {
    #[account(mut)]
    pub keeper: Signer<'info>,
    #[account(seeds = [b"registry"], bump = registry.bump)]
    pub registry: Box<Account<'info, Registry>>,
    #[account(mut, seeds = [b"vault", vault.xstock_mint.as_ref()], bump = vault.bump)]
    pub vault: Box<Account<'info, OverlayVault>>,
    #[account(init_if_needed, payer = keeper, space = 8 + ExitEpoch::INIT_SPACE,
        seeds = [b"epoch", vault.key().as_ref(), &vault.epoch_id.to_le_bytes()], bump)]
    pub exit_epoch: Box<Account<'info, ExitEpoch>>,
    pub system_program: Program<'info, System>,
}

pub fn close_epoch(ctx: Context<CloseEpoch>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let now = Clock::get()?.unix_timestamp;
    let v = &mut ctx.accounts.vault;
    require!(
        now - v.epoch_opened_ts >= v.params.epoch_len_secs as i64,
        CarreraError::TooSoon
    );
    require_nav_fresh(v)?;
    let e = &mut ctx.accounts.exit_epoch;
    if e.vault == Pubkey::default() {
        e.vault = v.key();
        e.id = v.epoch_id;
        e.bump = ctx.bumps.exit_epoch;
    }
    require!(!e.closed, CarreraError::WrongState);
    // `pending_exit_shares` counts every unsettled exit; closing while an earlier epoch is still
    // closed-but-unsettled would let the keeper size a second release for the same shares.
    require!(v.pending_exit_shares == e.shares_total, CarreraError::EpochNotSettled);
    let n = recompute_nav(v)?;
    let r = nav::redemption(
        e.shares_total,
        v.total_shares,
        n.depositor_qty,
        n.nav_usdc,
        v.price_e6,
        v.stock_decimals,
    )
    .ok_or(CarreraError::MathOverflow)?;
    e.stock_owed = r.stock_out;
    e.usdc_owed = r.usdc_out;
    if e.shares_total > 0 {
        e.stock_per_share_e6 = ((r.stock_out as u128) * 1_000_000 / (e.shares_total as u128)) as u64;
        e.usdc_per_share_e6 = ((r.usdc_out as u128) * 1_000_000 / (e.shares_total as u128)) as u64;
    }
    e.closed = true;
    if e.shares_total == 0 {
        e.settled = true;
    }
    emit!(EpochClosed {
        vault: v.key(),
        epoch_id: e.id,
        shares_total: e.shares_total,
        stock_owed: e.stock_owed,
        usdc_owed: e.usdc_owed,
    });
    v.epoch_id = v.epoch_id.checked_add(1).ok_or(CarreraError::MathOverflow)?;
    v.epoch_opened_ts = now;
    Ok(())
}

#[derive(Accounts)]
pub struct SettleEpoch<'info> {
    pub keeper: Signer<'info>,
    #[account(seeds = [b"registry"], bump = registry.bump)]
    pub registry: Box<Account<'info, Registry>>,
    #[account(mut, seeds = [b"vault", vault.xstock_mint.as_ref()], bump = vault.bump)]
    pub vault: Box<Account<'info, OverlayVault>>,
    #[account(mut, has_one = vault, seeds = [b"epoch", vault.key().as_ref(), &exit_epoch.id.to_le_bytes()], bump = exit_epoch.bump)]
    pub exit_epoch: Box<Account<'info, ExitEpoch>>,
    #[account(mut, seeds = [b"stock", vault.key().as_ref()], bump,
        token::mint = xstock_mint, token::token_program = stock_token_program)]
    pub stock_custody: Box<InterfaceAccount<'info, StockAccount>>,
    #[account(mut, seeds = [b"usdc", vault.key().as_ref()], bump)]
    pub usdc_buffer: Box<Account<'info, TokenAccount>>,
    #[account(mut, seeds = [b"redeem_stock", vault.key().as_ref()], bump,
        token::mint = xstock_mint, token::token_program = stock_token_program)]
    pub redeem_stock: Box<InterfaceAccount<'info, StockAccount>>,
    #[account(mut, seeds = [b"redeem_usdc", vault.key().as_ref()], bump)]
    pub redeem_usdc: Box<Account<'info, TokenAccount>>,
    /// Classic SPL Token: shares and USDC.
    pub token_program: Program<'info, Token>,
    #[account(constraint = xstock_mint.key() == vault.xstock_mint @ CarreraError::InvalidArgument)]
    pub xstock_mint: Box<InterfaceAccount<'info, StockMint>>,
    /// Token program owning the xStock mint (Token-2022 on mainnet).
    pub stock_token_program: Interface<'info, TokenInterface>,
}

pub fn settle_epoch<'info>(ctx: Context<'_, '_, '_, 'info, SettleEpoch<'info>>, venue_data: Vec<u8>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let vc = {
        let v = &ctx.accounts.vault;
        VenueCtx::new(ctx.remaining_accounts, v.to_account_info(), v.xstock_mint, v.bump, &venue_data)?
    };
    let v = &mut ctx.accounts.vault;
    let e = &mut ctx.accounts.exit_epoch;
    require!(e.closed, CarreraError::EpochNotClosed);
    require!(!e.settled, CarreraError::WrongState);
    require_nav_fresh(v)?;

    let stock_owed = e.stock_owed;
    let mut usdc_owed = e.usdc_owed;

    // Stock leg: must come from depositor stock and leave LTV inside the rebalance band (a partial
    // release for the exit leaves it at L plus rounding; debt below the dust threshold does not
    // count, spec dust tolerance).
    require!(depositor_qty(v) >= stock_owed, CarreraError::EpochUnderfunded);
    let remaining_value = stock_value(v, v.collateral_qty - stock_owed)?;
    require!(
        nav::settlement_ltv_ok(effective_debt(v), remaining_value, v.params.ltv_bps.saturating_add(v.params.rebalance_ltv_band_bps)),
        CarreraError::EpochUnderfunded
    );

    // USDC leg: the vault's own `usdc_buffer` first (real tokens, no venue call; it keeps its
    // cushion unless the epoch cannot otherwise be funded), then Kamino supply (Parked/Idle) or
    // free Phoenix collateral (Basis) for the remainder. USDC paid from the buffer is taken off
    // the parked accounting (never below zero): it may be transit or mock-era phantom parked.
    let in_buffer = ctx.accounts.usdc_buffer.amount;
    match v.vault_state() {
        VaultState::Parked | VaultState::Idle => {
            let (from_buffer, from_venue) =
                nav::usdc_leg_split(usdc_owed, in_buffer, v.parked_usdc, super::BUFFER_CUSHION_USDC).ok_or(CarreraError::EpochUnderfunded)?;
            if from_venue > 0 {
                kamino::withdraw_supplied_usdc(&vc, from_venue)?;
                v.parked_usdc -= from_venue;
            }
            v.parked_usdc = v.parked_usdc.saturating_sub(from_buffer);
        }
        VaultState::Basis => {
            let required_margin = mul_bps(stock_value(v, v.phoenix_short_qty)?, v.params.min_margin_bps)?;
            let free = v.phoenix_equity_usdc.saturating_sub(required_margin);
            let (from_buffer, from_venue) =
                nav::usdc_leg_split(usdc_owed, in_buffer, free, super::BUFFER_CUSHION_USDC).ok_or(CarreraError::EpochUnderfunded)?;
            if from_venue > 0 {
                crate::venues::phoenix::withdraw_collateral(&vc, from_venue)?;
                v.phoenix_equity_usdc -= from_venue;
                // Exit fee applies to what settlement draws from the live trade; it stays in NAV.
                let fee = mul_bps(from_venue, v.params.exit_fee_bps)?;
                usdc_owed -= fee;
                v.phoenix_equity_usdc += fee;
            }
            v.parked_usdc = v.parked_usdc.saturating_sub(from_buffer);
        }
        _ => return err!(CarreraError::WrongState),
    }

    if stock_owed > 0 {
        kamino::withdraw_collateral(&vc, stock_owed)?;
        v.collateral_qty -= stock_owed;
    }

    let seeds: &[&[u8]] = &[b"vault", v.xstock_mint.as_ref(), &[v.bump]];
    if stock_owed > 0 {
        token_2022::transfer_checked(
            CpiContext::new_with_signer(
                ctx.accounts.stock_token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.stock_custody.to_account_info(),
                    mint: ctx.accounts.xstock_mint.to_account_info(),
                    to: ctx.accounts.redeem_stock.to_account_info(),
                    authority: v.to_account_info(),
                },
                &[seeds],
            ),
            stock_owed,
            ctx.accounts.xstock_mint.decimals,
        )?;
    }
    if usdc_owed > 0 {
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.usdc_buffer.to_account_info(),
                    to: ctx.accounts.redeem_usdc.to_account_info(),
                    authority: v.to_account_info(),
                },
                &[seeds],
            ),
            usdc_owed,
        )?;
    }
    // Exiting shares leave `total_shares` now so remaining holders' share price is
    // unaffected; the escrowed tokens themselves are burned at `redeem`.
    if e.shares_total > 0 {
        v.total_shares = v.total_shares.checked_sub(e.shares_total).ok_or(CarreraError::MathOverflow)?;
        v.pending_exit_shares = v.pending_exit_shares.checked_sub(e.shares_total).ok_or(CarreraError::MathOverflow)?;
        e.usdc_owed = usdc_owed;
        e.usdc_per_share_e6 = ((usdc_owed as u128) * 1_000_000 / (e.shares_total as u128)) as u64;
    }
    e.settled = true;
    recompute_nav(v)?;
    emit!(EpochSettled { vault: v.key(), epoch_id: e.id, stock_paid: stock_owed, usdc_paid: usdc_owed });
    Ok(())
}

#[derive(Accounts)]
pub struct CrystalliseFee<'info> {
    pub keeper: Signer<'info>,
    #[account(seeds = [b"registry"], bump = registry.bump)]
    pub registry: Box<Account<'info, Registry>>,
    #[account(mut, seeds = [b"vault", vault.xstock_mint.as_ref()], bump = vault.bump, has_one = share_mint)]
    pub vault: Box<Account<'info, OverlayVault>>,
    #[account(mut)]
    pub share_mint: Box<Account<'info, Mint>>,
    /// Share token account owned by the registry admin (treasury).
    #[account(mut, token::mint = share_mint, constraint = treasury_shares.owner == registry.admin @ CarreraError::Unauthorized)]
    pub treasury_shares: Box<Account<'info, TokenAccount>>,
    pub token_program: Program<'info, Token>,
}

pub fn crystallise_fee(ctx: Context<CrystalliseFee>) -> Result<()> {
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let v = &mut ctx.accounts.vault;
    require_nav_fresh(v)?;
    let n = recompute_nav(v)?;
    let shares = nav::fee_shares(v.total_shares, n.share_price_stock_e6, v.high_water_e6, v.params.perf_fee_bps)
        .ok_or(CarreraError::MathOverflow)?;
    if shares > 0 {
        let seeds: &[&[u8]] = &[b"vault", v.xstock_mint.as_ref(), &[v.bump]];
        token::mint_to(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                MintTo {
                    mint: ctx.accounts.share_mint.to_account_info(),
                    to: ctx.accounts.treasury_shares.to_account_info(),
                    authority: v.to_account_info(),
                },
                &[seeds],
            ),
            shares,
        )?;
        v.total_shares = v.total_shares.checked_add(shares).ok_or(CarreraError::MathOverflow)?;
    }
    let n = recompute_nav(v)?;
    v.high_water_e6 = v.high_water_e6.max(n.share_price_stock_e6);
    emit!(FeeCrystallised { vault: v.key(), shares, high_water_e6: v.high_water_e6 });
    Ok(())
}
