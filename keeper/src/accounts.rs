//! Borsh decoders for the program accounts the keeper reads. Field order follows
//! docs/CONTRACT.md exactly. Pubkeys are decoded as raw 32-byte arrays so the
//! decoders do not depend on any solana crate feature.

use crate::ix::account_discriminator;
use anyhow::{anyhow, Result};
use borsh::{BorshDeserialize, BorshSerialize};
use solana_sdk::pubkey::Pubkey;

pub type Pk = [u8; 32];

#[allow(dead_code)]
pub fn pubkey(pk: &Pk) -> Pubkey {
    Pubkey::new_from_array(*pk)
}

#[derive(BorshDeserialize, BorshSerialize, Debug, Clone, Default)]
pub struct Registry {
    pub admin: Pk,
    pub guardian: Pk,
    pub keepers: [Pk; 4],
    pub keeper_count: u8,
    pub paused: bool,
    pub usdc_mint: Pk,
    pub borrow_apy_bps: u32,
    pub supply_apy_bps: u32,
    pub rates_slot: u64,
    pub bump: u8,
}

#[derive(BorshDeserialize, BorshSerialize, Debug, Clone, Default)]
pub struct VaultParams {
    pub ltv_bps: u32,
    pub min_margin_bps: u32,
    pub liq_ltv_bps: u32,
    pub emergency_ltv_bps: u32,
    pub enter_margin_bps: u32,
    pub exit_margin_bps: u32,
    pub carry_guard_margin_bps: u32,
    pub min_enter_funding_bps: u32,
    pub expected_hold_hours: u32,
    pub roundtrip_cost_bps: u32,
    pub funding_window: u8,
    pub max_swap_slippage_bps: u32,
    pub max_perp_slippage_bps: u32,
    pub max_index_dev_bps: u32,
    pub hedge_tol_bps: u32,
    pub size_band_bps: u32,
    pub rebalance_ltv_band_bps: u32,
    pub rebalance_margin_band_bps: u32,
    pub max_nav_age_slots: u64,
    pub epoch_len_secs: u64,
    pub perf_fee_bps: u32,
    pub exit_fee_bps: u32,
    pub deposit_cap_stock: u64,
    pub basis_cap_usdc: u64,
}

#[derive(BorshDeserialize, BorshSerialize, Debug, Clone, Default)]
pub struct RuleEvaluation {
    pub f_avg_bps: i64,
    pub parked_apy_bps: u32,
    pub r_bps: u32,
    pub hurdle_bps: i64,
    pub decision: u8,
    pub ts: i64,
}

#[derive(BorshDeserialize, BorshSerialize, Debug, Clone, Default)]
pub struct OverlayVault {
    pub xstock_mint: Pk,
    pub share_mint: Pk,
    pub tier: u8,
    pub params: VaultParams,
    pub state: u8,
    pub step: u8,
    pub market_open: bool,
    pub collateral_qty: u64,
    pub basis_spot_qty: u64,
    pub debt_usdc: u64,
    pub debt_b_usdc: u64,
    pub parked_usdc: u64,
    pub phoenix_equity_usdc: u64,
    pub phoenix_short_qty: u64,
    /// Hourly funding rate samples in bps × 1e6 (see README, "units").
    pub funding: [i64; 24],
    pub funding_head: u8,
    pub funding_samples: u8,
    pub last_funding_ts: i64,
    pub nav_usd_e6: u64,
    pub share_price_stock_e6: u64,
    pub price_e6: u64,
    pub nav_slot: u64,
    pub high_water_e6: u64,
    pub total_shares: u64,
    pub pending_exit_shares: u64,
    pub epoch_id: u64,
    pub epoch_opened_ts: i64,
    pub last_rule: RuleEvaluation,
    pub stock_decimals: u8,
    pub bump: u8,
}

#[derive(BorshDeserialize, BorshSerialize, Debug, Clone, Default)]
pub struct ExitEpoch {
    pub vault: Pk,
    pub id: u64,
    pub shares_total: u64,
    pub stock_owed: u64,
    pub usdc_owed: u64,
    pub stock_per_share_e6: u64,
    pub usdc_per_share_e6: u64,
    pub closed: bool,
    pub settled: bool,
    pub bump: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaultState {
    Idle,
    Parked,
    Winding,
    Basis,
    Unwinding,
    SizingUp,
    PartialUnwinding,
}

impl VaultState {
    pub fn from_u8(v: u8) -> Result<Self> {
        Ok(match v {
            0 => Self::Idle,
            1 => Self::Parked,
            2 => Self::Winding,
            3 => Self::Basis,
            4 => Self::Unwinding,
            5 => Self::SizingUp,
            6 => Self::PartialUnwinding,
            _ => return Err(anyhow!("unknown vault state {v}")),
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Parked => "Parked",
            Self::Winding => "Winding",
            Self::Basis => "Basis",
            Self::Unwinding => "Unwinding",
            Self::SizingUp => "SizingUp",
            Self::PartialUnwinding => "PartialUnwinding",
        }
    }
}

/// Scale of the on-chain funding samples: 1 bps/hour == 1_000_000.
pub const FUNDING_SCALE: i128 = 1_000_000;
pub const HOURS_PER_YEAR: i128 = 8760;

/// Decode an Anchor account: check the 8-byte discriminator, then Borsh the rest.
/// Trailing bytes (Anchor padding) are ignored.
pub fn decode<T: BorshDeserialize>(name: &str, data: &[u8]) -> Result<T> {
    if data.len() < 8 {
        return Err(anyhow!("{name}: account too short"));
    }
    if data[..8] != account_discriminator(name) {
        return Err(anyhow!("{name}: discriminator mismatch"));
    }
    let mut rest = &data[8..];
    T::deserialize(&mut rest).map_err(|e| anyhow!("{name}: borsh: {e}"))
}

impl OverlayVault {
    pub fn state(&self) -> Result<VaultState> {
        VaultState::from_u8(self.state)
    }

    /// 24-hour rolling average, annualised, in bps. Mirrors the program: mean(buffer) × 8760.
    pub fn f_avg_bps(&self) -> i64 {
        let n = (self.funding_samples as usize).min(24);
        if n == 0 {
            return 0;
        }
        let sum: i128 = self.funding[..n].iter().map(|&x| x as i128).sum();
        (sum * HOURS_PER_YEAR / (n as i128 * FUNDING_SCALE)) as i64
    }

    /// Mean of the newest `n` ring samples, annualised bps; `None` with fewer than `n` samples.
    /// The newest sample is at `funding_head − 1`.
    pub fn f_last_bps(&self, n: usize) -> Option<i64> {
        let len = self.funding.len();
        if n == 0 || (self.funding_samples as usize) < n || n > len {
            return None;
        }
        let sum: i128 = (1..=n).map(|k| self.funding[(self.funding_head as usize + len - k) % len] as i128).sum();
        Some((sum * HOURS_PER_YEAR / (n as i128 * FUNDING_SCALE)) as i64)
    }

    /// D8: the 3-sample entry average.
    pub fn f_3h_bps(&self) -> Option<i64> {
        self.f_last_bps(crate::rule::ENTRY_WINDOW as usize)
    }

    /// Value of all stock collateral in USDC base units (6 decimals).
    pub fn collateral_value_usdc(&self, stock_decimals: u8) -> u128 {
        self.collateral_qty as u128 * self.price_e6 as u128 / 10u128.pow(stock_decimals as u32)
    }

    /// Kamino-side LTV in bps: total USDC debt over collateral value. 0 when no collateral.
    pub fn ltv_bps(&self, stock_decimals: u8) -> u32 {
        let cv = self.collateral_value_usdc(stock_decimals);
        if cv == 0 {
            return 0;
        }
        let debt = self.debt_usdc as u128 + self.debt_b_usdc as u128;
        (debt * 10_000 / cv).min(u32::MAX as u128) as u32
    }

    /// Phoenix margin ratio in bps: subaccount equity over short notional. None when no short.
    pub fn margin_bps(&self, stock_decimals: u8) -> Option<u32> {
        self.margin_bps_with(stock_decimals, self.phoenix_equity_usdc)
    }

    /// Margin against the short notional for a given equity (the live trader-account read on
    /// the real build, the vault's cached figure otherwise).
    pub fn margin_bps_with(&self, stock_decimals: u8, equity_usdc: u64) -> Option<u32> {
        if self.phoenix_short_qty == 0 {
            return None;
        }
        let notional = self.phoenix_short_qty as u128 * self.price_e6 as u128 / 10u128.pow(stock_decimals as u32);
        if notional == 0 {
            return None;
        }
        Some((equity_usdc as u128 * 10_000 / notional).min(u32::MAX as u128) as u32)
    }

    pub fn nav_stale(&self, current_slot: u64) -> bool {
        current_slot.saturating_sub(self.nav_slot) > self.params.max_nav_age_slots
    }

    /// USDC to repay so that LTV returns to the tier's L. 0 if already at or below.
    pub fn debt_excess_usdc(&self, stock_decimals: u8) -> u64 {
        let cv = self.collateral_value_usdc(stock_decimals);
        let target = cv * self.params.ltv_bps as u128 / 10_000;
        let debt = self.debt_usdc as u128 + self.debt_b_usdc as u128;
        debt.saturating_sub(target).min(u64::MAX as u128) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_disc(name: &str, body: Vec<u8>) -> Vec<u8> {
        let mut v = account_discriminator(name).to_vec();
        v.extend(body);
        v
    }

    #[test]
    fn exit_epoch_decodes_from_hand_built_bytes() {
        // vault(32) id(8) shares_total(8) stock_owed(8) usdc_owed(8) sps(8) ups(8) closed(1) settled(1) bump(1)
        let mut body = vec![7u8; 32];
        body.extend(5u64.to_le_bytes());
        body.extend(1_000u64.to_le_bytes());
        body.extend(900u64.to_le_bytes());
        body.extend(42u64.to_le_bytes());
        body.extend(999_000u64.to_le_bytes());
        body.extend(1_500u64.to_le_bytes());
        body.push(1);
        body.push(0);
        body.push(254);
        // Anchor may pad accounts; extra trailing bytes must be tolerated.
        body.extend([0u8; 16]);
        let e: ExitEpoch = decode("ExitEpoch", &with_disc("ExitEpoch", body)).unwrap();
        assert_eq!(e.vault, [7u8; 32]);
        assert_eq!(e.id, 5);
        assert_eq!(e.shares_total, 1_000);
        assert_eq!(e.stock_owed, 900);
        assert_eq!(e.usdc_owed, 42);
        assert_eq!(e.stock_per_share_e6, 999_000);
        assert_eq!(e.usdc_per_share_e6, 1_500);
        assert!(e.closed);
        assert!(!e.settled);
        assert_eq!(e.bump, 254);
    }

    #[test]
    fn registry_round_trips_and_rejects_wrong_discriminator() {
        let r = Registry { keeper_count: 2, paused: true, borrow_apy_bps: 590, supply_apy_bps: 480, rates_slot: 99, bump: 1, ..Default::default() };
        let bytes = with_disc("Registry", borsh::to_vec(&r).unwrap());
        let back: Registry = decode("Registry", &bytes).unwrap();
        assert_eq!(back.borrow_apy_bps, 590);
        assert_eq!(back.supply_apy_bps, 480);
        assert!(back.paused);
        assert!(decode::<Registry>("OverlayVault", &bytes).is_err());
    }

    #[test]
    fn vault_layout_offsets_match_contract() {
        // state sits right after xstock_mint(32) + share_mint(32) + tier(1) + VaultParams.
        let params_len = borsh::to_vec(&VaultParams::default()).unwrap().len();
        assert_eq!(params_len, 4 * 19 + 1 + 8 * 2 + 8 * 2); // 19 u32, 1 u8, 2 u64 (nav age, epoch len), 2 u64 caps
        let mut v = OverlayVault { state: 3, step: 2, ..Default::default() };
        v.funding[0] = 1;
        let bytes = borsh::to_vec(&v).unwrap();
        let state_off = 32 + 32 + 1 + params_len;
        assert_eq!(bytes[state_off], 3);
        assert_eq!(bytes[state_off + 1], 2);
        let back: OverlayVault = decode("OverlayVault", &with_disc("OverlayVault", bytes)).unwrap();
        assert_eq!(back.state().unwrap(), VaultState::Basis);
        assert_eq!(back.funding[0], 1);
    }

    #[test]
    fn f_avg_annualises_mean_of_samples() {
        let mut v = OverlayVault::default();
        // 35% annualised funding == 3500 bps / 8760 h ≈ 0.39954 bps/h == 399_543 in scale-1e6 units.
        for i in 0..24 {
            v.funding[i] = 399_543;
        }
        v.funding_samples = 24;
        assert_eq!(v.f_avg_bps(), 3499);
        assert_eq!(v.f_3h_bps(), Some(3499));
        let mut three = v.clone();
        three.funding_samples = 3;
        three.funding_head = 3;
        three.funding[2] = 3 * three.funding[2]; // newest print triples
        let f3 = three.f_3h_bps().unwrap();
        assert!((f3 - 5832).abs() <= 2, "(3499·5)/3 ≈ 5832, got {f3}");
        three.funding_samples = 2;
        assert_eq!(three.f_3h_bps(), None);
        v.funding_samples = 0;
        assert_eq!(v.f_avg_bps(), 0);
    }

    #[test]
    fn ltv_and_margin_estimates() {
        let v = OverlayVault {
            collateral_qty: 100 * 10u64.pow(8), // 100 TSLAx at 8 decimals
            price_e6: 400_000_000,              // $400
            debt_usdc: 12_000_000_000,          // $12,000 → 30%
            phoenix_short_qty: 30 * 10u64.pow(8),
            phoenix_equity_usdc: 1_440_000_000, // $1,440 on $12,000 notional → 12%
            ..Default::default()
        };
        assert_eq!(v.collateral_value_usdc(8), 40_000_000_000);
        assert_eq!(v.ltv_bps(8), 3000);
        assert_eq!(v.margin_bps(8), Some(1200));
        let mut p = v.clone();
        p.params.ltv_bps = 2000;
        assert_eq!(p.debt_excess_usdc(8), 4_000_000_000);
    }
}
