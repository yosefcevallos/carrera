//! Live Phoenix equity: the vault's trader account decoded from chain (DECISIONS D6: the keeper
//! is the program's Phoenix oracle). `quote_lot_collateral` is the settled collateral in quote
//! lots, which on Phoenix's USDC exchange are USDC base units (proven in the fork: a deposit of
//! 34 495 304 base units shows as 34 495 304 quote lots). Funding accrued on open positions and
//! not yet settled into the collateral is exposed per position as
//! `accumulated_funding_for_active_position`.

use anyhow::{anyhow, Result};
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
    pub positions: u32,
}

impl TraderEquity {
    /// Equity including unsettled funding.
    pub fn equity_usdc(&self) -> i64 {
        self.collateral_usdc.saturating_add(self.pending_funding_usdc)
    }

    /// What a withdraw or a margin check may count on: settled collateral less any unsettled
    /// funding the trader owes. Unsettled funding owed *to* the trader is not counted.
    pub fn withdrawable_usdc(&self) -> u64 {
        self.collateral_usdc.saturating_add(self.pending_funding_usdc.min(0)).max(0) as u64
    }
}

/// Decode a trader account. The account bytes are copied into an 8-byte-aligned buffer first:
/// the Phoenix views borrow POD structs in place and reject misaligned input.
pub fn decode(data: &[u8]) -> Result<TraderEquity> {
    let words = data.len().div_ceil(8);
    let mut aligned: Vec<u64> = vec![0; words];
    // SAFETY: the u64 buffer is at least data.len() bytes long and outlives the slice.
    let bytes: &mut [u8] = unsafe { std::slice::from_raw_parts_mut(aligned.as_mut_ptr() as *mut u8, data.len()) };
    bytes.copy_from_slice(data);
    let t = Trader::try_from_account_bytes(bytes).map_err(|e| anyhow!("trader account: {e}"))?;
    let collateral_usdc = t.header.trader_state.quote_lot_collateral.as_inner();
    let mut pending_funding_usdc = 0i64;
    let mut positions = 0u32;
    for (_asset, p) in t.positions() {
        pending_funding_usdc = pending_funding_usdc.saturating_add(p.accumulated_funding_for_active_position().as_inner());
        positions += 1;
    }
    Ok(TraderEquity { collateral_usdc, pending_funding_usdc, positions })
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
    fn empty_trader_and_negative_collateral() {
        let e = decode(&fixture(0, &[])).unwrap();
        assert_eq!(e, TraderEquity { collateral_usdc: 0, pending_funding_usdc: 0, positions: 0 });
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
