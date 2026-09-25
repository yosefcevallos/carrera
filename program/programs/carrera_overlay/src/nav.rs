//! NAV, share price and redemption math (spec §4.2), adapted for Parked = Kamino supply.
//!
//! ```text
//! depositor_qty     = collateral_qty − basis_spot_qty
//! NAV_usd           = collateral_qty·p − debt − debt_b + phoenix_equity + parked_usdc
//! share_price_stock = NAV_usd / (p · total_shares)          // 1.000 at genesis
//! ```
//! Units: quantities in stock base units with `decimals`; USD values in USDC base
//! units (6 dp); `price_e6` is USD × 1e6 per whole stock.

pub const E6: u128 = 1_000_000;
pub const BPS: u128 = 10_000;

fn pow10(decimals: u8) -> u128 {
    10u128.pow(decimals as u32)
}

/// USDC value (6 dp) of `qty` stock base units at `price_e6`.
pub fn stock_value_usdc(qty: u64, price_e6: u64, decimals: u8) -> Option<u64> {
    (qty as u128)
        .checked_mul(price_e6 as u128)?
        .checked_div(pow10(decimals))?
        .try_into()
        .ok()
}

/// Stock base units purchasable with `usdc` at `price_e6` (rounded down).
pub fn stock_qty_from_usdc(usdc: u64, price_e6: u64, decimals: u8) -> Option<u64> {
    if price_e6 == 0 {
        return None;
    }
    (usdc as u128)
        .checked_mul(pow10(decimals))?
        .checked_div(price_e6 as u128)?
        .try_into()
        .ok()
}

pub fn mul_bps(amount: u64, bps: u32) -> Option<u64> {
    (amount as u128)
        .checked_mul(bps as u128)?
        .checked_div(BPS)?
        .try_into()
        .ok()
}

/// debt / value in bps. Zero value with zero debt is 0; zero value with debt is u32::MAX.
pub fn ratio_bps(numerator: u64, denominator: u64) -> u32 {
    if denominator == 0 {
        return if numerator == 0 { 0 } else { u32::MAX };
    }
    let r = (numerator as u128) * BPS / (denominator as u128);
    r.min(u32::MAX as u128) as u32
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NavInputs {
    pub collateral_qty: u64,
    pub basis_spot_qty: u64,
    pub debt_usdc: u64,
    pub debt_b_usdc: u64,
    pub phoenix_equity_usdc: u64,
    pub parked_usdc: u64,
    pub price_e6: u64,
    pub decimals: u8,
    pub total_shares: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Nav {
    pub nav_usdc: u64,
    pub share_price_stock_e6: u64,
    pub depositor_qty: u64,
}

pub fn compute_nav(i: &NavInputs) -> Option<Nav> {
    let collateral_value = stock_value_usdc(i.collateral_qty, i.price_e6, i.decimals)? as i128;
    let assets = collateral_value
        .checked_add(i.phoenix_equity_usdc as i128)?
        .checked_add(i.parked_usdc as i128)?;
    let liabilities = (i.debt_usdc as i128).checked_add(i.debt_b_usdc as i128)?;
    let nav = assets.checked_sub(liabilities)?.max(0);
    let nav_usdc: u64 = nav.try_into().ok()?;
    let depositor_qty = i.collateral_qty.checked_sub(i.basis_spot_qty)?;
    let share_price_stock_e6 = if i.total_shares == 0 {
        E6 as u64
    } else {
        // nav_usdc × 10^d × 1e6 / (price_e6 × total_shares)
        let num = (nav_usdc as u128)
            .checked_mul(pow10(i.decimals))?
            .checked_mul(E6)?;
        let den = (i.price_e6 as u128).checked_mul(i.total_shares as u128)?;
        if den == 0 {
            return None;
        }
        (num / den).try_into().ok()?
    };
    Some(Nav { nav_usdc, share_price_stock_e6, depositor_qty })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Redemption {
    pub stock_out: u64,
    pub usdc_out: u64,
}

/// Redemption of `shares` (spec §4.2):
/// ```text
/// φ = shares / total_shares
/// stock_out = φ · depositor_qty
/// usdc_out  = φ · NAV_usd − stock_out · p
/// if usdc_out < 0: stock_out −= |usdc_out| / p; usdc_out = 0
/// ```
pub fn redemption(
    shares: u64,
    total_shares: u64,
    depositor_qty: u64,
    nav_usdc: u64,
    price_e6: u64,
    decimals: u8,
) -> Option<Redemption> {
    if total_shares == 0 || shares == 0 {
        return Some(Redemption { stock_out: 0, usdc_out: 0 });
    }
    let shares = shares.min(total_shares);
    let stock_out: u64 = (shares as u128)
        .checked_mul(depositor_qty as u128)?
        .checked_div(total_shares as u128)?
        .try_into()
        .ok()?;
    let usdc_share: u64 = (shares as u128)
        .checked_mul(nav_usdc as u128)?
        .checked_div(total_shares as u128)?
        .try_into()
        .ok()?;
    let stock_value = stock_value_usdc(stock_out, price_e6, decimals)?;
    if usdc_share >= stock_value {
        Some(Redemption { stock_out, usdc_out: usdc_share - stock_value })
    } else {
        let deficit = stock_value - usdc_share;
        let less = stock_qty_from_usdc(deficit, price_e6, decimals)?;
        Some(Redemption { stock_out: stock_out.saturating_sub(less), usdc_out: 0 })
    }
}

/// Settlement LTV check with the dust tolerance applied by the caller
/// (`effective_debt`): a vault that carries only dust can always pay its stock out.
pub fn settlement_ltv_ok(effective_debt: u64, remaining_value_usdc: u64, ltv_bps: u32) -> bool {
    effective_debt == 0 || ratio_bps(effective_debt, remaining_value_usdc) <= ltv_bps
}

/// Performance-fee shares to mint when `share_price` is above `high_water`:
/// `total_shares × fee_bps × (sp − hw) / (10000 × sp)`.
pub fn fee_shares(total_shares: u64, share_price_e6: u64, high_water_e6: u64, fee_bps: u32) -> Option<u64> {
    if share_price_e6 <= high_water_e6 || share_price_e6 == 0 {
        return Some(0);
    }
    let gain = (share_price_e6 - high_water_e6) as u128;
    (total_shares as u128)
        .checked_mul(fee_bps as u128)?
        .checked_mul(gain)?
        .checked_div(BPS.checked_mul(share_price_e6 as u128)?)?
        .try_into()
        .ok()
}

/// Round a stock quantity down to whole Phoenix base lots (`lot` base units per lot; 0 = no lot
/// information, mock builds).
pub fn whole_lots(qty: u64, lot: u64) -> u64 {
    let lot = lot.max(1);
    qty / lot * lot
}

/// Hedge check: the short may differ from the spot by `tol_bps` of the spot or by one base lot,
/// whichever is larger. The sub-lot remainder of a spot leg cannot be shorted and stays unhedged.
pub fn hedge_ok(spot: u64, short: u64, tol_bps: u32, lot: u64) -> bool {
    if spot == 0 && short == 0 {
        return true;
    }
    let tol = ((spot as u128) * (tol_bps as u128) / BPS).min(u64::MAX as u128) as u64;
    spot.abs_diff(short) <= tol.max(lot)
}

/// Split an epoch's USDC leg between the vault's own `usdc_buffer` (real tokens, no venue call)
/// and the venue (Kamino supply, or free Phoenix collateral in Basis). The buffer keeps `floor`
/// unless the epoch cannot otherwise be funded; `None` when buffer + venue cannot cover it.
/// Returns `(from_buffer, from_venue)`.
pub fn usdc_leg_split(owed: u64, buffer: u64, available: u64, floor: u64) -> Option<(u64, u64)> {
    let from_buffer = owed.min(buffer.saturating_sub(floor));
    let rest = owed - from_buffer;
    if rest <= available {
        return Some((from_buffer, rest));
    }
    let from_buffer = owed.min(buffer);
    let rest = owed - from_buffer;
    (rest <= available).then_some((from_buffer, rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mainnet AAPL after unwind + repay: 0.0029 USDC of dust debt against 876 base
    /// units of remaining collateral (~10 000 bps) must not block a full exit.
    #[test]
    fn settlement_ignores_dust_debt() {
        const DUST: u64 = 10_000;
        let (collateral, stock_owed, debt, price_e6, ltv) = (2_959_805u64, 2_958_929u64, 2_944u64, 335_853_313u64, 2_500u32);
        let remaining_value = stock_value_usdc(collateral - stock_owed, price_e6, 8).unwrap();
        assert!(ratio_bps(debt, remaining_value) > ltv, "raw ratio really is above the tier LTV");
        let effective = if debt < DUST { 0 } else { debt };
        assert!(settlement_ltv_ok(effective, remaining_value, ltv));
        // Above dust the check still bites.
        assert!(!settlement_ltv_ok(DUST, remaining_value, ltv));
        // And a healthy vault passes on the ratio itself.
        assert!(settlement_ltv_ok(1_000_000, 10_000_000, ltv));
    }

    const DEC: u8 = 8; // xStock decimals
    const ONE: u64 = 100_000_000; // 1.00000000 stock
    const PRICE: u64 = 412_000_000; // $412.00

    fn genesis(qty: u64) -> NavInputs {
        NavInputs {
            collateral_qty: qty,
            basis_spot_qty: 0,
            debt_usdc: 0,
            debt_b_usdc: 0,
            phoenix_equity_usdc: 0,
            parked_usdc: 0,
            price_e6: PRICE,
            decimals: DEC,
            total_shares: qty,
        }
    }

    #[test]
    fn stock_value_and_back() {
        assert_eq!(stock_value_usdc(ONE, PRICE, DEC), Some(412_000_000));
        assert_eq!(stock_value_usdc(10 * ONE, PRICE, DEC), Some(4_120_000_000));
        assert_eq!(stock_qty_from_usdc(412_000_000, PRICE, DEC), Some(ONE));
        assert_eq!(stock_qty_from_usdc(0, PRICE, DEC), Some(0));
        assert_eq!(stock_qty_from_usdc(1, 0, DEC), None);
    }

    #[test]
    fn share_price_is_one_at_genesis() {
        let nav = compute_nav(&genesis(10 * ONE)).unwrap();
        assert_eq!(nav.nav_usdc, 4_120_000_000);
        assert_eq!(nav.share_price_stock_e6, 1_000_000);
        assert_eq!(nav.depositor_qty, 10 * ONE);
    }

    #[test]
    fn empty_vault_share_price_is_one() {
        let nav = compute_nav(&genesis(0)).unwrap();
        assert_eq!(nav.share_price_stock_e6, 1_000_000);
        assert_eq!(nav.nav_usdc, 0);
    }

    #[test]
    fn parked_loan_is_nav_neutral_and_basis_spot_is_not_depositor_stock() {
        // Borrow 30% and park it: NAV unchanged.
        let mut i = genesis(10 * ONE);
        i.debt_usdc = 1_236_000_000; // 30% of $4120
        i.parked_usdc = 1_236_000_000;
        let nav = compute_nav(&i).unwrap();
        assert_eq!(nav.nav_usdc, 4_120_000_000);
        assert_eq!(nav.share_price_stock_e6, 1_000_000);
        // Now in Basis: loan became 3 stock of spot plus D_b in Phoenix.
        i.parked_usdc = 0;
        i.basis_spot_qty = 3 * ONE;
        i.collateral_qty = 13 * ONE;
        i.debt_b_usdc = 370_800_000; // 30% of D
        i.phoenix_equity_usdc = 370_800_000;
        let nav = compute_nav(&i).unwrap();
        assert_eq!(nav.nav_usdc, 4_120_000_000);
        assert_eq!(nav.depositor_qty, 10 * ONE);
    }

    #[test]
    fn earned_usdc_raises_share_price() {
        let mut i = genesis(10 * ONE);
        i.parked_usdc = 41_200_000; // +$41.20 = +1% of $4120
        let nav = compute_nav(&i).unwrap();
        assert_eq!(nav.share_price_stock_e6, 1_010_000);
    }

    #[test]
    fn nav_floors_at_zero() {
        let mut i = genesis(ONE);
        i.debt_usdc = 10_000_000_000;
        let nav = compute_nav(&i).unwrap();
        assert_eq!(nav.nav_usdc, 0);
        assert_eq!(nav.share_price_stock_e6, 0);
    }

    #[test]
    fn redemption_pro_rata_with_earned_usdc() {
        // 10 stock, $41.20 earned. Redeem half.
        let r = redemption(5 * ONE, 10 * ONE, 10 * ONE, 4_161_200_000, PRICE, DEC).unwrap();
        assert_eq!(r.stock_out, 5 * ONE);
        assert_eq!(r.usdc_out, 20_600_000);
    }

    #[test]
    fn redemption_at_genesis_pays_stock_only() {
        let r = redemption(ONE, 10 * ONE, 10 * ONE, 4_120_000_000, PRICE, DEC).unwrap();
        assert_eq!(r, Redemption { stock_out: ONE, usdc_out: 0 });
    }

    #[test]
    fn redemption_negative_usdc_reduces_stock_leg() {
        // NAV below stock value (e.g. round-trip costs early in life): $4120 stock, NAV $4100.
        let r = redemption(10 * ONE, 10 * ONE, 10 * ONE, 4_100_000_000, PRICE, DEC).unwrap();
        assert_eq!(r.usdc_out, 0);
        // deficit $20 → 20/412 stock less
        let less = stock_qty_from_usdc(20_000_000, PRICE, DEC).unwrap();
        assert_eq!(r.stock_out, 10 * ONE - less);
        assert!(r.stock_out < 10 * ONE);
    }

    #[test]
    fn redemption_edge_cases() {
        assert_eq!(redemption(0, 10, 10, 100, PRICE, DEC).unwrap(), Redemption { stock_out: 0, usdc_out: 0 });
        assert_eq!(redemption(5, 0, 0, 0, PRICE, DEC).unwrap(), Redemption { stock_out: 0, usdc_out: 0 });
        // Over-redeem is clamped to total.
        let r = redemption(20 * ONE, 10 * ONE, 10 * ONE, 4_120_000_000, PRICE, DEC).unwrap();
        assert_eq!(r.stock_out, 10 * ONE);
    }

    #[test]
    fn ratio_bps_handles_zero() {
        assert_eq!(ratio_bps(0, 0), 0);
        assert_eq!(ratio_bps(1, 0), u32::MAX);
        assert_eq!(ratio_bps(30, 100), 3000);
    }

    #[test]
    fn fee_shares_only_above_high_water() {
        assert_eq!(fee_shares(1_000_000, 1_000_000, 1_000_000, 1500), Some(0));
        assert_eq!(fee_shares(1_000_000, 900_000, 1_000_000, 1500), Some(0));
        // 10% gain, 15% fee → 1.5% of shares
        assert_eq!(fee_shares(1_000_000, 1_100_000, 1_000_000, 1500), Some(13_636));
    }

    #[test]
    fn short_size_rounds_to_whole_lots_and_hedge_tolerates_the_remainder() {
        // Mainnet TSLA, 25 Sep 2026: wind_step(1) bought 1 028 932 base units; the market's
        // base_lots_decimals = 3 → 100 000 base units per lot → 10 lots = 1 000 000 shortable.
        let (qty, lot) = (1_028_932u64, 100_000u64);
        assert_eq!(whole_lots(qty, lot), 1_000_000);
        // The fill check runs on the lot-rounded size: 10 lots filled ≥ 99.7 % of 1 000 000.
        let min_fill = whole_lots(qty, lot) as u128 * (10_000 - 30) / 10_000;
        assert!(1_000_000 >= min_fill as u64);
        // Against the raw quantity it would fail (the live SlippageExceeded): 1 000 000 < 1 025 845.
        assert!(1_000_000 < qty as u128 * (10_000 - 30) / 10_000);
        // The 28 932-unit remainder (2.8 %) is inside one lot, so the commit's hedge check passes.
        assert!(hedge_ok(qty, 1_000_000, 50, lot));
        assert!(!hedge_ok(qty, 900_000, 50, lot), "a whole lot short of the spot is not tolerated");
        assert!(hedge_ok(30_444_913, 30_400_000, 50, lot));
        // No lot information (mock builds): plain bps tolerance, no rounding.
        assert_eq!(whole_lots(qty, 0), qty);
        assert!(hedge_ok(10_000, 9_960, 50, 0));
        assert!(!hedge_ok(10_000, 9_940, 50, 0));
        assert!(whole_lots(99_999, lot) == 0, "less than a lot is not shortable");
    }

    #[test]
    fn epoch_usdc_leg_comes_from_the_buffer_first() {
        // QQQ, 25 Sep 2026: mock-era accounting owed 17 762 with 15 225 "parked" that never
        // reached Kamino, while the seeded buffer held 100 000 real base units.
        assert_eq!(usdc_leg_split(17_762, 100_000, 15_225, 50_000), Some((17_762, 0)));
        // The floor is kept when the venue can cover the rest.
        assert_eq!(usdc_leg_split(60_000, 100_000, 20_000, 50_000), Some((50_000, 10_000)));
        // ... and given up when that is the only way to fund the epoch.
        assert_eq!(usdc_leg_split(60_000, 100_000, 5_000, 50_000), Some((60_000, 0)));
        assert_eq!(usdc_leg_split(105_000, 100_000, 5_000, 50_000), Some((100_000, 5_000)));
        // A buffer below the floor contributes nothing while the venue suffices.
        assert_eq!(usdc_leg_split(30_000, 20_000, 100_000, 50_000), Some((0, 30_000)));
        // Nothing owed; and genuinely underfunded.
        assert_eq!(usdc_leg_split(0, 0, 0, 50_000), Some((0, 0)));
        assert_eq!(usdc_leg_split(200_000, 100_000, 50_000, 50_000), None);
    }
}
