//! Allocation rule (docs/DECISIONS.md D2). Pure integer math, evaluated on-chain.
//!
//! All rates are annualised bps on basis notional `D`.
//!
//! ```text
//! cost_apy           = roundtrip_cost_bps × 8760 / expected_hold_hours
//! hurdle_from_parked = s + L·r + cost_apy        // primary loan interest cancels
//! hurdle_from_idle   = r + L·r + cost_apy        // no loan in Idle, Basis pays all of r
//! carry_ok           = s ≥ r + carry_guard_margin
//! ```

use crate::state::{VaultParams, VaultState};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Decision {
    None,
    ToBasis,
    ToParked,
    ToIdle,
}

impl Decision {
    pub fn as_u8(self) -> u8 {
        match self {
            Decision::None => crate::state::DECISION_NONE,
            Decision::ToBasis => crate::state::DECISION_TO_BASIS,
            Decision::ToParked => crate::state::DECISION_TO_PARKED,
            Decision::ToIdle => crate::state::DECISION_TO_IDLE,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RuleInputs {
    pub state: VaultState,
    /// 24h rolling funding average, annualised bps. `None` when no samples.
    pub f_avg_bps: Option<i64>,
    pub samples: u8,
    /// Kamino USDC supply APY, bps.
    pub s_bps: u32,
    /// Kamino USDC borrow APY, bps.
    pub r_bps: u32,
    pub market_open: bool,
    pub paused: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuleOutput {
    pub decision: Decision,
    /// The hurdle relevant to the decision (parked variant unless Idle → Basis).
    pub hurdle_bps: i64,
    pub f_avg_bps: i64,
}

pub fn cost_apy_bps(p: &VaultParams) -> i64 {
    if p.expected_hold_hours == 0 {
        return i64::MAX / 4;
    }
    (p.roundtrip_cost_bps as i64) * 8760 / (p.expected_hold_hours as i64)
}

pub fn l_times_r_bps(p: &VaultParams, r_bps: u32) -> i64 {
    (p.ltv_bps as i64) * (r_bps as i64) / 10_000
}

pub fn hurdle_from_parked(p: &VaultParams, s_bps: u32, r_bps: u32) -> i64 {
    s_bps as i64 + l_times_r_bps(p, r_bps) + cost_apy_bps(p)
}

pub fn hurdle_from_idle(p: &VaultParams, r_bps: u32) -> i64 {
    r_bps as i64 + l_times_r_bps(p, r_bps) + cost_apy_bps(p)
}

pub fn carry_ok(p: &VaultParams, s_bps: u32, r_bps: u32) -> bool {
    (s_bps as i64) >= (r_bps as i64) + (p.carry_guard_margin_bps as i64)
}

pub fn evaluate(p: &VaultParams, i: &RuleInputs) -> RuleOutput {
    let h_parked = hurdle_from_parked(p, i.s_bps, i.r_bps);
    let h_idle = hurdle_from_idle(p, i.r_bps);
    let carry = carry_ok(p, i.s_bps, i.r_bps);
    let f = i.f_avg_bps.unwrap_or(0);
    let enough_samples = i.f_avg_bps.is_some() && i.samples >= p.funding_window;
    let enter = p.enter_margin_bps as i64;
    let exit = p.exit_margin_bps as i64;
    let floor = p.min_enter_funding_bps as i64;

    let can_enter = |hurdle: i64| -> bool {
        !i.paused && i.market_open && enough_samples && f > hurdle + enter && f >= floor
    };

    let (decision, hurdle) = match i.state {
        VaultState::Parked => {
            if !carry {
                (Decision::ToIdle, h_parked)
            } else if can_enter(h_parked) {
                (Decision::ToBasis, h_parked)
            } else {
                (Decision::None, h_parked)
            }
        }
        VaultState::Idle => {
            if can_enter(h_idle) {
                (Decision::ToBasis, h_idle)
            } else if carry && !i.paused {
                (Decision::ToParked, h_parked)
            } else {
                (Decision::None, h_idle)
            }
        }
        VaultState::Basis => {
            if !i.market_open {
                (Decision::None, h_parked)
            } else if carry && f < h_parked - exit {
                (Decision::ToParked, h_parked)
            } else if !carry && f < h_idle - exit {
                (Decision::ToIdle, h_idle)
            } else {
                (Decision::None, if carry { h_parked } else { h_idle })
            }
        }
        VaultState::Winding | VaultState::Unwinding => (Decision::None, h_parked),
    };

    RuleOutput { decision, hurdle_bps: hurdle, f_avg_bps: f }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spec defaults: L=30%, 200/100 bps hysteresis, 50 bps carry guard,
    /// 450 bps floor, 720 h hold, 60 bps round trip → cost_apy = 730.
    fn params() -> VaultParams {
        VaultParams {
            ltv_bps: 3000,
            enter_margin_bps: 200,
            exit_margin_bps: 100,
            carry_guard_margin_bps: 50,
            min_enter_funding_bps: 450,
            expected_hold_hours: 720,
            roundtrip_cost_bps: 60,
            funding_window: 24,
            ..Default::default()
        }
    }

    fn inputs(state: VaultState, f: i64, s: u32, r: u32) -> RuleInputs {
        RuleInputs {
            state,
            f_avg_bps: Some(f),
            samples: 24,
            s_bps: s,
            r_bps: r,
            market_open: true,
            paused: false,
        }
    }

    #[test]
    fn worked_numbers_from_spec() {
        let p = params();
        assert_eq!(cost_apy_bps(&p), 730);
        assert_eq!(l_times_r_bps(&p, 590), 177);
        // s = 650 → hurdle_from_parked = 650 + 177 + 730 = 1557
        assert_eq!(hurdle_from_parked(&p, 650, 590), 1557);
        // Idle variant pays the full borrow rate: 590 + 177 + 730 = 1497
        assert_eq!(hurdle_from_idle(&p, 590), 1497);
    }

    #[test]
    fn guard_fires_at_spec_rates_supply_480_borrow_590() {
        let p = params();
        assert!(!carry_ok(&p, 480, 590));
        let out = evaluate(&p, &inputs(VaultState::Parked, 3500, 480, 590));
        assert_eq!(out.decision, Decision::ToIdle, "guard must win over funding");
    }

    #[test]
    fn guard_respects_margin_boundary() {
        let p = params();
        assert!(!carry_ok(&p, 639, 590));
        assert!(carry_ok(&p, 640, 590));
    }

    #[test]
    fn parked_enters_basis_above_hurdle_plus_margin() {
        let p = params();
        // hurdle 1557 + enter 200 = 1757
        let stay = evaluate(&p, &inputs(VaultState::Parked, 1757, 650, 590));
        assert_eq!(stay.decision, Decision::None);
        let go = evaluate(&p, &inputs(VaultState::Parked, 1758, 650, 590));
        assert_eq!(go.decision, Decision::ToBasis);
        assert_eq!(go.hurdle_bps, 1557);
    }

    #[test]
    fn parked_needs_full_window_of_samples() {
        let p = params();
        let mut i = inputs(VaultState::Parked, 3000, 650, 590);
        i.samples = 23;
        assert_eq!(evaluate(&p, &i).decision, Decision::None);
        i.f_avg_bps = None;
        i.samples = 24;
        assert_eq!(evaluate(&p, &i).decision, Decision::None);
    }

    #[test]
    fn parked_needs_market_open_and_not_paused() {
        let p = params();
        let mut i = inputs(VaultState::Parked, 3000, 650, 590);
        i.market_open = false;
        assert_eq!(evaluate(&p, &i).decision, Decision::None);
        i.market_open = true;
        i.paused = true;
        assert_eq!(evaluate(&p, &i).decision, Decision::None);
    }

    #[test]
    fn idle_enters_basis_on_idle_hurdle() {
        let p = params();
        // Idle hurdle 1497 + 200 = 1697. Carry is bad (s=480) so the alternative is None.
        let stay = evaluate(&p, &inputs(VaultState::Idle, 1697, 480, 590));
        assert_eq!(stay.decision, Decision::None);
        let go = evaluate(&p, &inputs(VaultState::Idle, 1698, 480, 590));
        assert_eq!(go.decision, Decision::ToBasis);
        assert_eq!(go.hurdle_bps, 1497);
    }

    #[test]
    fn idle_prefers_basis_over_parked_when_both_hold() {
        let p = params();
        let out = evaluate(&p, &inputs(VaultState::Idle, 5000, 650, 590));
        assert_eq!(out.decision, Decision::ToBasis);
    }

    #[test]
    fn idle_parks_when_carry_positive_and_funding_low() {
        let p = params();
        let out = evaluate(&p, &inputs(VaultState::Idle, 500, 650, 590));
        assert_eq!(out.decision, Decision::ToParked);
        let mut i = inputs(VaultState::Idle, 500, 650, 590);
        i.paused = true;
        assert_eq!(evaluate(&p, &i).decision, Decision::None);
        // Parking does not need the market to be open.
        let mut i = inputs(VaultState::Idle, 500, 650, 590);
        i.market_open = false;
        assert_eq!(evaluate(&p, &i).decision, Decision::ToParked);
    }

    #[test]
    fn idle_stays_idle_when_carry_negative_and_funding_low() {
        let p = params();
        let out = evaluate(&p, &inputs(VaultState::Idle, 500, 480, 590));
        assert_eq!(out.decision, Decision::None);
    }

    #[test]
    fn floor_450_blocks_entry_even_above_a_tiny_hurdle() {
        let mut p = params();
        // Make the hurdle tiny so the floor is the binding constraint.
        p.roundtrip_cost_bps = 0;
        p.enter_margin_bps = 0;
        let s = 100;
        let r = 40; // carry ok: 100 >= 40 + 50
        assert_eq!(hurdle_from_parked(&p, s, r), 100 + 12);
        let below = evaluate(&p, &inputs(VaultState::Parked, 449, s, r));
        assert_eq!(below.decision, Decision::None);
        let at = evaluate(&p, &inputs(VaultState::Parked, 450, s, r));
        assert_eq!(at.decision, Decision::ToBasis);
    }

    #[test]
    fn basis_exits_to_parked_below_hurdle_minus_margin_with_hysteresis() {
        let p = params();
        // hurdle 1557 − exit 100 = 1457. Between 1457 and 1757 nothing happens.
        let hold_low = evaluate(&p, &inputs(VaultState::Basis, 1457, 650, 590));
        assert_eq!(hold_low.decision, Decision::None);
        let hold_high = evaluate(&p, &inputs(VaultState::Basis, 1757, 650, 590));
        assert_eq!(hold_high.decision, Decision::None);
        let out = evaluate(&p, &inputs(VaultState::Basis, 1456, 650, 590));
        assert_eq!(out.decision, Decision::ToParked);
    }

    #[test]
    fn basis_exits_to_idle_when_carry_negative() {
        let p = params();
        // Idle hurdle 1497 − 100 = 1397.
        let hold = evaluate(&p, &inputs(VaultState::Basis, 1397, 480, 590));
        assert_eq!(hold.decision, Decision::None);
        let out = evaluate(&p, &inputs(VaultState::Basis, 1396, 480, 590));
        assert_eq!(out.decision, Decision::ToIdle);
        assert_eq!(out.hurdle_bps, 1497);
    }

    #[test]
    fn basis_never_leaves_by_rule_while_market_closed() {
        let p = params();
        let mut i = inputs(VaultState::Basis, 0, 480, 590);
        i.market_open = false;
        assert_eq!(evaluate(&p, &i).decision, Decision::None);
    }

    #[test]
    fn transitional_states_decide_nothing() {
        let p = params();
        assert_eq!(evaluate(&p, &inputs(VaultState::Winding, 9000, 650, 590)).decision, Decision::None);
        assert_eq!(evaluate(&p, &inputs(VaultState::Unwinding, 0, 480, 590)).decision, Decision::None);
    }

    #[test]
    fn break_even_from_one_pager_tsla_35pct_enters() {
        let p = params();
        // 35% funding clears the ≈21% entry level from spec v0.4.1 (with s=650 here).
        let out = evaluate(&p, &inputs(VaultState::Parked, 3500, 650, 590));
        assert_eq!(out.decision, Decision::ToBasis);
    }
}
