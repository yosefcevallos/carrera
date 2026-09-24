use crate::errors::CarreraError;
use crate::events::{FundingRecorded, KaminoRatesRecorded, NavRefreshed};
use crate::ring;
use crate::state::{OverlayVault, Registry};
use crate::venues;
use anchor_lang::prelude::*;

use super::{recompute_nav, require_keeper, vault_key};

/// Minimum spacing between funding samples. 59 min so an hourly keeper with jitter is not rejected.
#[cfg(not(feature = "mock-venues"))]
pub const MIN_FUNDING_SPACING_SECS: i64 = 3540;
/// Mock builds let tests record a full window without waiting a day.
#[cfg(feature = "mock-venues")]
pub const MIN_FUNDING_SPACING_SECS: i64 = 0;

#[derive(Accounts)]
pub struct RecordFunding<'info> {
    /// Anyone may crank; the signer is slot 0 so keeper encoding is uniform.
    pub signer: Signer<'info>,
    #[account(seeds = [b"registry"], bump = registry.bump)]
    pub registry: Account<'info, Registry>,
    #[account(mut, seeds = [b"vault", vault.xstock_mint.as_ref()], bump = vault.bump)]
    pub vault: Account<'info, OverlayVault>,
    /// CHECK: Hawkeye view account for the vault's Phoenix market; decoded in `venues::hawkeye`.
    pub hawkeye_view: UncheckedAccount<'info>,
}

pub fn record_funding(ctx: Context<RecordFunding>, mock_rate_bps_hourly: Option<i64>) -> Result<()> {
    // Keeper-supplied values are only accepted from a registered keeper.
    if mock_rate_bps_hourly.is_some() {
        require_keeper(&ctx.accounts.registry, &ctx.accounts.signer.key())?;
    }
    let now = Clock::get()?.unix_timestamp;
    let rate = venues::hawkeye::read_funding(&ctx.accounts.hawkeye_view, mock_rate_bps_hourly)?;
    let v: &mut OverlayVault = &mut ctx.accounts.vault;
    require!(now - v.last_funding_ts >= MIN_FUNDING_SPACING_SECS, CarreraError::TooSoon);
    let OverlayVault { funding, funding_head, funding_samples, .. } = v;
    ring::push(funding, funding_head, funding_samples, rate);
    v.last_funding_ts = now;
    let f_avg = ring::f_avg_bps(&v.funding, v.funding_samples).unwrap_or(0);
    emit!(FundingRecorded {
        vault: vault_key(v),
        rate_bps_e6_hourly: rate,
        f_avg_bps: f_avg,
        samples: v.funding_samples,
    });
    Ok(())
}

#[derive(Accounts)]
pub struct RecordKaminoRates<'info> {
    pub signer: Signer<'info>,
    #[account(mut, seeds = [b"registry"], bump = registry.bump)]
    pub registry: Account<'info, Registry>,
    /// CHECK: Kamino USDC reserve account; decoded in `venues::kamino::read_rates`.
    pub kamino_reserve: UncheckedAccount<'info>,
}

pub fn record_kamino_rates(
    ctx: Context<RecordKaminoRates>,
    mock_borrow_bps: Option<u32>,
    mock_supply_bps: Option<u32>,
) -> Result<()> {
    let mock = match (mock_borrow_bps, mock_supply_bps) {
        (Some(b), Some(s)) => Some((b, s)),
        (None, None) => None,
        _ => return err!(CarreraError::InvalidArgument),
    };
    if mock.is_some() {
        require_keeper(&ctx.accounts.registry, &ctx.accounts.signer.key())?;
    }
    let (borrow, supply) = venues::kamino::read_rates(&ctx.accounts.kamino_reserve, mock)?;
    let r = &mut ctx.accounts.registry;
    r.borrow_apy_bps = borrow;
    r.supply_apy_bps = supply;
    r.rates_slot = Clock::get()?.slot;
    emit!(KaminoRatesRecorded { borrow_apy_bps: borrow, supply_apy_bps: supply });
    Ok(())
}

#[derive(Accounts)]
pub struct RefreshNav<'info> {
    pub signer: Signer<'info>,
    #[account(seeds = [b"registry"], bump = registry.bump)]
    pub registry: Account<'info, Registry>,
    #[account(mut, seeds = [b"vault", vault.xstock_mint.as_ref()], bump = vault.bump)]
    pub vault: Account<'info, OverlayVault>,
    /// CHECK: the oracle Kamino uses for the xStock reserve; decoded in `venues::kamino::read_price`.
    pub oracle: UncheckedAccount<'info>,
}

pub fn refresh_nav(ctx: Context<RefreshNav>, mock_price_e6: Option<u64>) -> Result<()> {
    if mock_price_e6.is_some() {
        require_keeper(&ctx.accounts.registry, &ctx.accounts.signer.key())?;
    }
    let price = venues::kamino::read_price(&ctx.accounts.oracle, mock_price_e6)?;
    let v = &mut ctx.accounts.vault;
    v.price_e6 = price;
    let n = recompute_nav(v)?;
    v.nav_slot = Clock::get()?.slot;
    emit!(NavRefreshed {
        vault: vault_key(v),
        nav_usd_e6: n.nav_usdc,
        share_price_stock_e6: n.share_price_stock_e6,
        price_e6: price,
    });
    Ok(())
}

/// Mock-only: simulate USDC earned by the deployed loan. `leg` 0 = parked
/// (Kamino supply interest), 1 = Phoenix equity (funding received). The
/// matching USDC must be minted into the vault's `usdc_buffer` by the test
/// harness before it can be paid out by `settle_epoch`.
pub fn mock_accrue(ctx: Context<super::KeeperVault>, usdc: u64, leg: u8) -> Result<()> {
    require!(venues::MOCK, CarreraError::MockNotAllowed);
    require_keeper(&ctx.accounts.registry, &ctx.accounts.keeper.key())?;
    let v = &mut ctx.accounts.vault;
    match leg {
        0 => v.parked_usdc = v.parked_usdc.checked_add(usdc).ok_or(CarreraError::MathOverflow)?,
        1 => v.phoenix_equity_usdc = v.phoenix_equity_usdc.checked_add(usdc).ok_or(CarreraError::MathOverflow)?,
        _ => return err!(CarreraError::InvalidArgument),
    }
    recompute_nav(v)?;
    Ok(())
}
