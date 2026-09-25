use anchor_lang::prelude::*;

pub const MAX_KEEPERS: usize = 4;
pub const FUNDING_WINDOW: usize = 24;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum VaultState {
    Idle = 0,
    Parked = 1,
    Winding = 2,
    Basis = 3,
    Unwinding = 4,
    /// Basis → Basis with a larger position: `size_up_start` → `size_up_step(1..3)` → `size_up_commit`.
    SizingUp = 5,
    /// Basis → Basis with a smaller position: `unwind_partial_start` → `unwind_partial_step(1..3)` → `unwind_partial_commit`.
    PartialUnwinding = 6,
}

impl VaultState {
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => Self::Idle,
            1 => Self::Parked,
            2 => Self::Winding,
            3 => Self::Basis,
            4 => Self::Unwinding,
            5 => Self::SizingUp,
            6 => Self::PartialUnwinding,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum UnwindReason {
    Rule = 0,
    ExitDemand = 1,
    Emergency = 2,
}

impl UnwindReason {
    pub fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            0 => Self::Rule,
            1 => Self::ExitDemand,
            2 => Self::Emergency,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum ExitStatus {
    Open = 0,
    Settled = 1,
    Redeemed = 2,
    Cancelled = 3,
}

/// Rule decisions, stored in `RuleEvaluation.decision`.
pub const DECISION_NONE: u8 = 0;
/// Residual debt (USDC base units, 0.01 USDC) tolerated after an unwind; never blocks settlement.
pub const DEBT_DUST_USDC: u64 = 10_000;
pub const DECISION_TO_BASIS: u8 = 1;
pub const DECISION_TO_PARKED: u8 = 2;
pub const DECISION_TO_IDLE: u8 = 3;

#[account]
#[derive(InitSpace)]
pub struct Registry {
    pub admin: Pubkey,
    pub guardian: Pubkey,
    pub keepers: [Pubkey; MAX_KEEPERS],
    pub keeper_count: u8,
    pub paused: bool,
    pub usdc_mint: Pubkey,
    /// Kamino USDC reserve rates, annualised bps, written by `record_kamino_rates`.
    pub borrow_apy_bps: u32,
    pub supply_apy_bps: u32,
    pub rates_slot: u64,
    pub bump: u8,
}

impl Registry {
    pub fn is_keeper(&self, key: &Pubkey) -> bool {
        self.keepers[..self.keeper_count as usize].contains(key)
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, InitSpace, Debug, PartialEq, Eq)]
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
    /// W: funding samples required before Basis may be entered (default 24).
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

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default, InitSpace, Debug)]
pub struct RuleEvaluation {
    pub f_avg_bps: i64,
    pub parked_apy_bps: u32,
    pub r_bps: u32,
    pub hurdle_bps: i64,
    pub decision: u8,
    pub ts: i64,
}

#[account]
#[derive(InitSpace)]
pub struct OverlayVault {
    pub xstock_mint: Pubkey,
    pub share_mint: Pubkey,
    pub tier: u8,
    pub params: VaultParams,
    pub state: u8,
    pub step: u8,
    pub market_open: bool,
    /// Stock held as Kamino collateral: depositor stock plus basis spot.
    pub collateral_qty: u64,
    pub basis_spot_qty: u64,
    /// Primary loan D.
    pub debt_usdc: u64,
    /// Secondary loan D_b, Basis only.
    pub debt_b_usdc: u64,
    /// USDC supplied on Kamino (Parked), plus residual USDC in Idle.
    pub parked_usdc: u64,
    pub phoenix_equity_usdc: u64,
    pub phoenix_short_qty: u64,
    /// Hourly funding samples, hourly rate in bps × 1e6 (bps_e6).
    pub funding: [i64; FUNDING_WINDOW],
    pub funding_head: u8,
    pub funding_samples: u8,
    pub last_funding_ts: i64,
    /// NAV cache: USDC base units (6 dp); share price in stock, 1e6 = 1.000.
    pub nav_usd_e6: u64,
    pub share_price_stock_e6: u64,
    /// Oracle price, USD × 1e6 per whole stock.
    pub price_e6: u64,
    pub nav_slot: u64,
    pub high_water_e6: u64,
    pub total_shares: u64,
    pub pending_exit_shares: u64,
    pub epoch_id: u64,
    pub epoch_opened_ts: i64,
    pub last_rule: RuleEvaluation,
    /// Decimals of the xStock mint (shares use the same). Not in CONTRACT.md v1; needed for USD math.
    pub stock_decimals: u8,
    pub bump: u8,
}

impl OverlayVault {
    pub fn vault_state(&self) -> VaultState {
        VaultState::from_u8(self.state).unwrap_or(VaultState::Idle)
    }
    pub fn total_debt(&self) -> u64 {
        self.debt_usdc.saturating_add(self.debt_b_usdc)
    }
}

#[account]
#[derive(InitSpace)]
pub struct ExitRequest {
    pub vault: Pubkey,
    pub user: Pubkey,
    pub nonce: u64,
    pub shares: u64,
    pub epoch_id: u64,
    pub status: u8,
    pub bump: u8,
}

#[account]
#[derive(InitSpace)]
pub struct ExitEpoch {
    pub vault: Pubkey,
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
