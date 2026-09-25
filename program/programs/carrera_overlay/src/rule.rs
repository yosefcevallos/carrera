//! Allocation rule (docs/DECISIONS.md D8). Pure integer math, evaluated on-chain.
//!
//! All rates are annualised bps on basis notional `D`. The vault pays Kamino on both loans, so
//! the funding it needs to break even is `be = r + L·r`.
//!
//! ```text
//! f_3h  = mean of the newest 3 hourly funding samples, annualised
//! f_24h = mean of the whole ring, annualised
//! to BASIS  : Idle/Parked && f_3h > be + enter_margin && f_3h >= min_enter_funding
//!             && f_24h >= be − exit_margin (D8.1: no entry the exit rule would undo)
//!             && samples >= 3 && market_open && !paused
//! to IDLE   : Basis && f_24h < be − exit_margin   (Parked instead when the carry guard allows)
//! carry_ok  = s ≥ r + carry_guard_margin
//! ```
//! `expected_hold_hours` and `roundtrip_cost_bps` stay in `VaultParams` (account layout) but
//! no longer enter the rule (D8 dropped cost amortisation from the entry).

use crate::state::{VaultParams, VaultState};

/// Samples the entry looks at (D8).
pub const ENTRY_WINDOW: u8 = 3;

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
    /// Mean of the newest 3 samples, annualised bps. `None` with fewer than 3 samples.
    pub f_3h_bps: Option<i64>,
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
    /// The level the decision compared against: break-even `r + L·r`.
    pub hurdle_bps: i64,
    pub f_avg_bps: i64,
    pub f_3h_bps: i64,
    pub be_bps: i64,
}

/// `L·r` in bps, rounded to nearest.
pub fn l_times_r_bps(p: &VaultParams, r_bps: u32) -> i64 {
    ((p.ltv_bps as i64) * (r_bps as i64) + 5_000) / 10_000
}

/// Break-even funding: the primary loan's interest plus the secondary loan's, `r + L·r`.
pub fn break_even_bps(p: &VaultParams, r_bps: u32) -> i64 {
    r_bps as i64 + l_times_r_bps(p, r_bps)
}

pub fn carry_ok(p: &VaultParams, s_bps: u32, r_bps: u32) -> bool {
    (s_bps as i64) >= (r_bps as i64) + (p.carry_guard_margin_bps as i64)
}

pub fn evaluate(p: &VaultParams, i: &RuleInputs) -> RuleOutput {
    let be = break_even_bps(p, i.r_bps);
    let carry = carry_ok(p, i.s_bps, i.r_bps);
    let f24 = i.f_avg_bps.unwrap_or(0);
    let f3 = i.f_3h_bps.unwrap_or(0);
    let enter = p.enter_margin_bps as i64;
    let exit = p.exit_margin_bps as i64;
    let floor = p.min_enter_funding_bps as i64;
    // D8.1: never enter a position the exit rule would close next hour.
    let can_enter =
        !i.paused && i.market_open && i.f_3h_bps.is_some() && i.samples >= ENTRY_WINDOW && f3 > be + enter && f3 >= floor && f24 >= be - exit;

    let decision = match i.state {
        VaultState::Parked => {
            if !carry {
                Decision::ToIdle
            } else if can_enter {
                Decision::ToBasis
            } else {
                Decision::None
            }
        }
        VaultState::Idle => {
            if can_enter {
                Decision::ToBasis
            } else if carry && !i.paused {
                Decision::ToParked
            } else {
                Decision::None
            }
        }
        VaultState::Basis => {
            if !i.market_open || i.f_avg_bps.is_none() {
                Decision::None
            } else if f24 < be - exit {
                if carry {
                    Decision::ToParked
                } else {
                    Decision::ToIdle
                }
            } else {
                Decision::None
            }
        }
        VaultState::Winding | VaultState::Unwinding | VaultState::SizingUp | VaultState::PartialUnwinding => Decision::None,
    };

    RuleOutput { decision, hurdle_bps: be, f_avg_bps: f24, f_3h_bps: f3, be_bps: be }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// D8 numbers: L = 39 % (TSLA per D7), r = 579 → be = 579 + 226 = 805; enter above 1005,
    /// exit below 705. 200/100 bps hysteresis, 50 bps carry guard, 450 bps floor.
    fn params() -> VaultParams {
        VaultParams {
            ltv_bps: 3900,
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

    const R: u32 = 579;
    const S_GOOD: u32 = 650; // carry ok: 650 ≥ 579 + 50
    const S_BAD: u32 = 480;

    fn inputs(state: VaultState, f3: i64, f24: i64, s: u32, r: u32) -> RuleInputs {
        RuleInputs { state, f_avg_bps: Some(f24), f_3h_bps: Some(f3), samples: 24, s_bps: s, r_bps: r, market_open: true, paused: false }
    }

    #[test]
    fn break_even_from_decisions_d8() {
        let p = params();
        assert_eq!(l_times_r_bps(&p, R), 226);
        assert_eq!(break_even_bps(&p, R), 805);
        let out = evaluate(&p, &inputs(VaultState::Parked, 0, 0, S_GOOD, R));
        assert_eq!((out.be_bps, out.hurdle_bps), (805, 805));
    }

    #[test]
    fn guard_fires_at_supply_480_borrow_579() {
        let p = params();
        assert!(!carry_ok(&p, S_BAD, R));
        let out = evaluate(&p, &inputs(VaultState::Parked, 3500, 3500, S_BAD, R));
        assert_eq!(out.decision, Decision::ToIdle, "guard must win over funding");
        assert!(!carry_ok(&p, 628, R));
        assert!(carry_ok(&p, 629, R));
    }

    #[test]
    fn enters_on_the_3h_average_above_1005() {
        let p = params();
        // The 24h average only has to clear the exit line (705) for entry: 800 here.
        let stay = evaluate(&p, &inputs(VaultState::Parked, 1005, 800, S_GOOD, R));
        assert_eq!(stay.decision, Decision::None);
        let go = evaluate(&p, &inputs(VaultState::Parked, 1006, 800, S_GOOD, R));
        assert_eq!(go.decision, Decision::ToBasis);
        assert_eq!(go.hurdle_bps, 805);
        assert_eq!((go.f_3h_bps, go.f_avg_bps), (1006, 800));
        // Same level from Idle (be does not depend on where the vault starts).
        assert_eq!(evaluate(&p, &inputs(VaultState::Idle, 1006, 800, S_BAD, R)).decision, Decision::ToBasis);
        assert_eq!(evaluate(&p, &inputs(VaultState::Idle, 1005, 800, S_BAD, R)).decision, Decision::None);
        // D8.1 (the live SPY case): a 3h spike with the 24h average under the exit line stays out.
        assert_eq!(evaluate(&p, &inputs(VaultState::Idle, 6698, -51, S_BAD, R)).decision, Decision::None);
        assert_eq!(evaluate(&p, &inputs(VaultState::Parked, 1006, 704, S_GOOD, R)).decision, Decision::None);
    }

    #[test]
    fn a_high_24h_average_does_not_enter_when_the_last_three_hours_are_low() {
        let p = params();
        assert_eq!(evaluate(&p, &inputs(VaultState::Parked, 900, 3500, S_GOOD, R)).decision, Decision::None);
    }

    #[test]
    fn entry_needs_three_samples() {
        let p = params();
        let mut i = inputs(VaultState::Parked, 3000, 3000, S_GOOD, R);
        i.samples = 2;
        i.f_3h_bps = None;
        assert_eq!(evaluate(&p, &i).decision, Decision::None);
        i.samples = 3;
        i.f_3h_bps = Some(3000);
        assert_eq!(evaluate(&p, &i).decision, Decision::ToBasis);
    }

    #[test]
    fn entry_needs_market_open_and_not_paused() {
        let p = params();
        let mut i = inputs(VaultState::Parked, 3000, 3000, S_GOOD, R);
        i.market_open = false;
        assert_eq!(evaluate(&p, &i).decision, Decision::None);
        i.market_open = true;
        i.paused = true;
        assert_eq!(evaluate(&p, &i).decision, Decision::None);
    }

    #[test]
    fn idle_parks_when_carry_positive_and_funding_low() {
        let p = params();
        assert_eq!(evaluate(&p, &inputs(VaultState::Idle, 500, 500, S_GOOD, R)).decision, Decision::ToParked);
        let mut i = inputs(VaultState::Idle, 500, 500, S_GOOD, R);
        i.paused = true;
        assert_eq!(evaluate(&p, &i).decision, Decision::None);
        // Parking does not need the market to be open.
        let mut i = inputs(VaultState::Idle, 500, 500, S_GOOD, R);
        i.market_open = false;
        assert_eq!(evaluate(&p, &i).decision, Decision::ToParked);
        assert_eq!(evaluate(&p, &inputs(VaultState::Idle, 500, 500, S_BAD, R)).decision, Decision::None);
    }

    #[test]
    fn floor_450_blocks_entry_when_break_even_is_tiny() {
        let mut p = params();
        p.enter_margin_bps = 0;
        let (s, r) = (100, 40); // be = 40 + 16 = 56, carry ok
        assert_eq!(break_even_bps(&p, r), 56);
        assert_eq!(evaluate(&p, &inputs(VaultState::Parked, 449, 449, s, r)).decision, Decision::None);
        assert_eq!(evaluate(&p, &inputs(VaultState::Parked, 450, 450, s, r)).decision, Decision::ToBasis);
    }

    #[test]
    fn basis_exits_on_the_24h_average_below_705_with_hysteresis() {
        let p = params();
        // Between 705 and 1005 nothing happens, whatever the 3h print does.
        assert_eq!(evaluate(&p, &inputs(VaultState::Basis, 200, 705, S_GOOD, R)).decision, Decision::None);
        assert_eq!(evaluate(&p, &inputs(VaultState::Basis, 5000, 1005, S_GOOD, R)).decision, Decision::None);
        let out = evaluate(&p, &inputs(VaultState::Basis, 5000, 704, S_GOOD, R));
        assert_eq!(out.decision, Decision::ToParked, "carry allows Parked");
        assert_eq!(out.hurdle_bps, 805);
        let out = evaluate(&p, &inputs(VaultState::Basis, 5000, 704, S_BAD, R));
        assert_eq!(out.decision, Decision::ToIdle, "no carry: straight to Idle");
    }

    #[test]
    fn a_low_3h_print_does_not_exit_while_the_24h_average_holds() {
        let p = params();
        assert_eq!(evaluate(&p, &inputs(VaultState::Basis, 0, 900, S_GOOD, R)).decision, Decision::None);
    }

    #[test]
    fn basis_never_leaves_by_rule_while_market_closed() {
        let p = params();
        let mut i = inputs(VaultState::Basis, 0, 0, S_BAD, R);
        i.market_open = false;
        assert_eq!(evaluate(&p, &i).decision, Decision::None);
    }

    #[test]
    fn transitional_states_decide_nothing() {
        let p = params();
        assert_eq!(evaluate(&p, &inputs(VaultState::Winding, 9000, 9000, S_GOOD, R)).decision, Decision::None);
        assert_eq!(evaluate(&p, &inputs(VaultState::Unwinding, 0, 0, S_BAD, R)).decision, Decision::None);
        assert_eq!(evaluate(&p, &inputs(VaultState::SizingUp, 9000, 9000, S_GOOD, R)).decision, Decision::None);
        assert_eq!(evaluate(&p, &inputs(VaultState::PartialUnwinding, 0, 0, S_BAD, R)).decision, Decision::None);
    }

    #[test]
    fn tsla_35pct_enters_after_three_samples() {
        let p = params();
        let mut i = inputs(VaultState::Parked, 3500, 3500, S_GOOD, R);
        i.samples = 3;
        assert_eq!(evaluate(&p, &i).decision, Decision::ToBasis);
    }
}
