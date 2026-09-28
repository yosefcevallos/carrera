//! Live Phoenix equity: the vault's trader account decoded from chain (DECISIONS D6: the keeper
//! is the program's Phoenix oracle). `quote_lot_collateral` is the settled collateral in quote
//! lots, which on Phoenix's USDC exchange are USDC base units (proven in the fork: a deposit of
//! 34 495 304 base units shows as 34 495 304 quote lots). Funding accrued on open positions and
//! not yet settled into the collateral is exposed per position as
//! `accumulated_funding_for_active_position`.

use anyhow::{anyhow, Result};
use phoenix_rise_accounts::perp_asset_map::PerpAssetMap;
use phoenix_rise_accounts::trader::Trader;

/// `TraderHeader.trader_state.quote_lot_collateral`: after discriminant 8, SequenceNumber 16,
/// key 32, authority 32.
#[cfg(test)]
pub const QUOTE_LOT_COLLATERAL_OFFSET: usize = 88;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct TraderEquity {
    /// Settled collateral, USDC base units (may be negative after a loss).
    pub collateral_usdc: i64,
    /// Funding accrued on open positions and not yet settled into the collateral, USDC base
    /// units, positive when owed to the trader.
    pub pending_funding_usdc: i64,
    /// Mark-to-market gain or loss on open positions at the market's finalised mark price,
    /// USDC base units; 0 when decoded without the PerpAssetMap.
    pub unrealized_pnl_usdc: i64,
    pub positions: u32,
}

impl TraderEquity {
    /// Subaccount equity: settled collateral plus unsettled funding plus unrealised PnL. This
    /// is what NAV carries for the hedge leg.
    pub fn equity_usdc(&self) -> i64 {
        self.collateral_usdc.saturating_add(self.pending_funding_usdc).saturating_add(self.unrealized_pnl_usdc)
    }

    /// What a withdraw or a margin check may count on: settled collateral less any unsettled
    /// funding the trader owes and any open loss. Gains not yet settled are not counted.
    pub fn withdrawable_usdc(&self) -> u64 {
        self.collateral_usdc.saturating_add(self.pending_funding_usdc.min(0)).saturating_add(self.unrealized_pnl_usdc.min(0)).max(0) as u64
    }
}

/// Decode a trader account on its own: pending funding counts only what Phoenix has already
/// accumulated onto the position, not what the market index has accrued since.
pub fn decode(data: &[u8]) -> Result<TraderEquity> {
    decode_with_market(data, None, 0)
}

/// Decode a trader account and, given the exchange's PerpAssetMap, add the funding each
/// position has accrued since its snapshot of the market's cumulative funding index (the
/// Phoenix SDK's `unsettled_funding`, what settles into collateral on the next interaction
/// with the position) and the position's mark-to-market PnL. `mark_price_e6` is the stock
/// price the vault's NAV already uses for the spot legs (USD × 1e6), so the hedge nets out
/// against them exactly; the map's own finalised mark is used only when no price is given.
/// The account bytes are copied into 8-byte-aligned buffers first: the Phoenix views borrow
/// POD structs in place and reject misaligned input.
pub fn decode_with_market(data: &[u8], perp_asset_map: Option<&[u8]>, mark_price_e6: u64) -> Result<TraderEquity> {
    let trader_buf = aligned(data);
    let t = Trader::try_from_account_bytes(trader_buf.as_slice()).map_err(|e| anyhow!("trader account: {e}"))?;
    let map_buf = perp_asset_map.map(aligned);
    let map = match map_buf.as_ref() {
        Some(b) => Some(PerpAssetMap::try_from_account_bytes(b.as_slice()).map_err(|e| anyhow!("perp asset map: {e}"))?),
        None => None,
    };
    let collateral_usdc = t.header.trader_state.quote_lot_collateral.as_inner();
    let mut pending_funding_usdc = 0i64;
    let mut unrealized_pnl_usdc = 0i64;
    let mut positions = 0u32;
    for (asset, p) in t.positions() {
        pending_funding_usdc = pending_funding_usdc.saturating_add(p.accumulated_funding_for_active_position().as_inner());
        if let Some(map) = map.as_ref() {
            let m = market_marks(map, asset as u32, mark_price_e6)?;
            let since = unsettled_funding(m.cumulative_funding_rate, p.cumulative_funding_snapshot().as_inner(), p.base_lot_position().as_inner());
            pending_funding_usdc = pending_funding_usdc.saturating_add(since);
            let pnl = unrealized_pnl(p.base_lot_position().as_inner(), p.virtual_quote_lot_position().as_inner(), m.mark_quote_lots_per_base_lot);
            if std::env::var_os("CARRERA_PHOENIX_DEBUG").is_some() {
                eprintln!(
                    "phoenix position asset {asset}: base_lots {} virtual_quote {} snapshot {} cumulative_rate {} mark_ticks {} tick_size {} base_lot_decimals {} mark_qlpbl {} pnl {pnl} since {since}",
                    p.base_lot_position().as_inner(), p.virtual_quote_lot_position().as_inner(), p.cumulative_funding_snapshot().as_inner(),
                    m.cumulative_funding_rate, m.mark_ticks, m.tick_size, m.base_lot_decimals, m.mark_quote_lots_per_base_lot
                );
            }
            unrealized_pnl_usdc = unrealized_pnl_usdc.saturating_add(pnl);
        }
        positions += 1;
    }
    Ok(TraderEquity { collateral_usdc, pending_funding_usdc, unrealized_pnl_usdc, positions })
}

struct MarketMarks {
    /// The market's cumulative funding index, quote lots per base lot.
    cumulative_funding_rate: i64,
    /// The finalised mark price scaled into quote lots per base lot (ticks × tick size).
    mark_quote_lots_per_base_lot: u64,
    mark_ticks: u64,
    tick_size: u64,
    base_lot_decimals: i8,
}

fn market_marks(map: &PerpAssetMap, asset_id: u32, mark_price_e6: u64) -> Result<MarketMarks> {
    for entry in map.iter() {
        let entry = entry.map_err(|e| anyhow!("perp asset map entry: {e}"))?;
        let sp = entry.metadata.static_market_params();
        if sp.asset_id() == asset_id {
            let mark_ticks = entry.metadata.finalized_mark_price().as_inner();
            let tick_size = sp.tick_size.as_inner();
            return Ok(MarketMarks {
                cumulative_funding_rate: entry.metadata.funding_accumulator().cumulative_funding_rate.as_inner(),
                mark_quote_lots_per_base_lot: mark_quote_lots_per_base_lot(mark_price_e6, sp.base_lot_decimals, mark_ticks, tick_size),
                mark_ticks,
                tick_size,
                base_lot_decimals: sp.base_lot_decimals,
            });
        }
    }
    Err(anyhow!("asset {asset_id} not in the perp asset map"))
}

/// Mark price in quote lots (USDC base units) per base lot. A base lot is `10^-base_lot_decimals`
/// of the stock, so a USD × 1e6 price divides by `10^base_lot_decimals`; without a price, the
/// map's finalised mark (ticks × tick size) is used, which is 0 on markets that do not
/// finalise one on chain.
pub fn mark_quote_lots_per_base_lot(price_e6: u64, base_lot_decimals: i8, mark_ticks: u64, tick_size: u64) -> u64 {
    if price_e6 > 0 {
        let scale = 10u64.pow(base_lot_decimals.max(0) as u32);
        price_e6 / scale
    } else {
        mark_ticks.saturating_mul(tick_size)
    }
}

/// Mark-to-market PnL of a position, quote lots: the SDK's `base_lots × mark + virtual_quote`
/// (the virtual quote position is the cash paid or received when the position was built, so
/// a short carries a positive virtual quote and gains as the mark falls).
pub fn unrealized_pnl(base_lots: i64, virtual_quote_lots: i64, mark_quote_lots_per_base_lot: u64) -> i64 {
    if base_lots == 0 {
        return 0;
    }
    let value = (base_lots as i128).saturating_mul(mark_quote_lots_per_base_lot as i128);
    value.saturating_add(virtual_quote_lots as i128).clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

/// Funding accrued on a position since its snapshot, quote lots (USDC base units): the SDK's
/// `-(cumulative_funding_rate - snapshot) * base_lots`. Positive is owed to the trader, so a
/// short (negative base lots) earns while the index rises.
pub fn unsettled_funding(cumulative_rate: i64, snapshot: i64, base_lots: i64) -> i64 {
    let diff = (cumulative_rate as i128) - (snapshot as i128);
    let v = -(diff * base_lots as i128);
    v.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

/// Copy `data` into an 8-byte-aligned buffer (as a `Vec<u64>` viewed as bytes).
fn aligned(data: &[u8]) -> AlignedBytes {
    let mut words: Vec<u64> = vec![0; data.len().div_ceil(8)];
    // SAFETY: the u64 buffer is at least data.len() bytes long.
    let bytes: &mut [u8] = unsafe { std::slice::from_raw_parts_mut(words.as_mut_ptr() as *mut u8, data.len()) };
    bytes.copy_from_slice(data);
    AlignedBytes { words, len: data.len() }
}

struct AlignedBytes {
    words: Vec<u64>,
    len: usize,
}

impl AlignedBytes {
    fn as_slice(&self) -> &[u8] {
        // SAFETY: `words` holds at least `len` bytes and lives as long as `self`.
        unsafe { std::slice::from_raw_parts(self.words.as_ptr() as *const u8, self.len) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phoenix_rise_accounts::PhoenixAccount;

    /// A trader account with `collateral` settled and `n` positions each carrying `funding`
    /// unsettled: header (224 bytes), position map header (len n, capacity n + 1), entries of 40.
    fn fixture(collateral: i64, funding: &[i64]) -> Vec<u8> {
        let capacity = funding.len() + 1;
        let mut d = vec![0u8; 224 + 16 + 40 * capacity];
        d[..8].copy_from_slice(&PhoenixAccount::Trader.discriminant());
        d[QUOTE_LOT_COLLATERAL_OFFSET..QUOTE_LOT_COLLATERAL_OFFSET + 8].copy_from_slice(&collateral.to_le_bytes());
        d[96..100].copy_from_slice(&0x3eu32.to_le_bytes()); // capabilities, as after onboarding
        d[224..232].copy_from_slice(&(funding.len() as u64).to_le_bytes());
        d[232..240].copy_from_slice(&(capacity as u64).to_le_bytes());
        for (i, f) in funding.iter().enumerate() {
            let o = 240 + 40 * i;
            d[o..o + 8].copy_from_slice(&(42u64 + i as u64).to_le_bytes()); // asset id
            d[o + 8..o + 16].copy_from_slice(&(-304i64).to_le_bytes()); // base lots short
            // position raw: base 8, virtual quote 8, funding snapshot 8, sequence 1, accumulated funding i56 (7 LE bytes)
            d[o + 33..o + 40].copy_from_slice(&f.to_le_bytes()[..7]);
        }
        d
    }

    #[test]
    fn decodes_collateral_and_pending_funding() {
        let e = decode(&fixture(34_495_304, &[-1_250, 300])).unwrap();
        assert_eq!(e.collateral_usdc, 34_495_304);
        assert_eq!(e.pending_funding_usdc, -950);
        assert_eq!(e.positions, 2);
        assert_eq!(e.equity_usdc(), 34_494_354);
        // Withdrawable counts funding owed by the trader, never funding owed to it.
        assert_eq!(e.withdrawable_usdc(), 34_495_304 - 950);
        let owed_to_us = decode(&fixture(1_000_000, &[5_000])).unwrap();
        assert_eq!(owed_to_us.withdrawable_usdc(), 1_000_000);
        assert_eq!(owed_to_us.equity_usdc(), 1_005_000);
    }

    #[test]
    fn unsettled_funding_follows_the_sdk_sign_convention() {
        // Short 304 base lots; the index rose by 12 quote lots per base lot since the snapshot:
        // longs paid, the short receives 3 648.
        assert_eq!(unsettled_funding(1_012, 1_000, -304), 3_648);
        // Same move on a long: it pays.
        assert_eq!(unsettled_funding(1_012, 1_000, 304), -3_648);
        // Falling index: the short pays.
        assert_eq!(unsettled_funding(990, 1_000, -304), -3_040);
        assert_eq!(unsettled_funding(1_000, 1_000, -304), 0);
        assert_eq!(unsettled_funding(1_000, 1_000, 0), 0);
    }

    #[test]
    fn mark_from_the_vault_price_or_the_map() {
        // TSLA at $357.39 with 3 base-lot decimals (a lot is 0.001 TSLA): 357 390 quote lots per lot,
        // the same as the map's 35 739 ticks × 10.
        assert_eq!(mark_quote_lots_per_base_lot(357_390_000, 3, 0, 10), 357_390);
        assert_eq!(mark_quote_lots_per_base_lot(0, 3, 35_739, 10), 357_390);
        let e = TraderEquity { collateral_usdc: 1_000, pending_funding_usdc: 50, unrealized_pnl_usdc: -300, positions: 1 };
        assert_eq!(e.equity_usdc(), 750);
        assert_eq!(e.withdrawable_usdc(), 700);
    }

    #[test]
    fn unrealized_pnl_matches_the_sdk_cases() {
        // Long 10 lots bought for 800, marked at 100: +200. Short 10 sold for 1 200: +200.
        assert_eq!(unrealized_pnl(10, -800, 100), 200);
        assert_eq!(unrealized_pnl(-10, 1_200, 100), 200);
        // Short sold at 100/lot, mark up to 110: −100. Flat: 0.
        assert_eq!(unrealized_pnl(-10, 1_000, 110), -100);
        assert_eq!(unrealized_pnl(0, 5_000, 100), 0);
    }

    #[test]
    fn empty_trader_and_negative_collateral() {
        let e = decode(&fixture(0, &[])).unwrap();
        assert_eq!(e, TraderEquity { collateral_usdc: 0, pending_funding_usdc: 0, unrealized_pnl_usdc: 0, positions: 0 });
        assert_eq!(decode(&fixture(-7, &[])).unwrap().withdrawable_usdc(), 0);
    }

    #[test]
    fn rejects_foreign_accounts() {
        assert!(decode(&[0u8; 300]).is_err(), "wrong discriminant");
        assert!(decode(&fixture(1, &[])[..100]).is_err(), "truncated");
        // Misaligned input is fine: the bytes are copied into an aligned buffer first.
        let f = fixture(9, &[]);
        let mut shifted = vec![0u8];
        shifted.extend_from_slice(&f);
        assert_eq!(decode(&shifted[1..]).unwrap().collateral_usdc, 9);
    }
}
