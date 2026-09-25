//! Mirror of the on-chain allocation rule (docs/DECISIONS.md D8). The keeper uses it only
//! to decide which crank to send; the program re-evaluates in the same transaction and
//! rejects anything the rule does not permit.
//!
//! `be = r + L·r` (both loans' interest). Enter Basis when the 3h funding average clears
//! `be + enter_margin` (and the 450 bps floor, with ≥ 3 samples, market open, not paused);
//! leave Basis when the 24h average drops below `be − exit_margin`.

use crate::accounts::{VaultParams, VaultState};

/// Samples the entry looks at (D8).
pub const ENTRY_WINDOW: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    None,
    ToBasis,
    ToParked,
    ToIdle,
}

#[derive(Clone, Copy, Debug)]
pub struct Hurdles {
    /// Break-even funding `r + L·r`: the level both entry and exit compare against.
    pub be_bps: i64,
    pub enter_bps: i64,
    pub exit_bps: i64,
    /// Kamino supply covers borrow plus the carry-guard margin.
    pub carry_ok: bool,
}

pub fn hurdles(p: &VaultParams, supply_bps: u32, borrow_bps: u32) -> Hurdles {
    let l_r_bps = (p.ltv_bps as i64 * borrow_bps as i64 + 5_000) / 10_000;
    let be_bps = borrow_bps as i64 + l_r_bps;
    Hurdles {
        be_bps,
        enter_bps: be_bps + p.enter_margin_bps as i64,
        exit_bps: be_bps - p.exit_margin_bps as i64,
        carry_ok: supply_bps as i64 >= borrow_bps as i64 + p.carry_guard_margin_bps as i64,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Inputs {
    pub state: VaultState,
    /// 24h average, annualised bps (0 with no samples).
    pub f_avg_bps: i64,
    /// Mean of the newest 3 samples, annualised bps; `None` with fewer than 3.
    pub f_3h_bps: Option<i64>,
    pub samples: u8,
    pub supply_bps: u32,
    pub borrow_bps: u32,
    pub market_open: bool,
    pub paused: bool,
}

pub fn evaluate(p: &VaultParams, i: &Inputs) -> Decision {
    let h = hurdles(p, i.supply_bps, i.borrow_bps);
    let f3 = i.f_3h_bps.unwrap_or(0);
    let can_enter = !i.paused
        && i.market_open
        && i.f_3h_bps.is_some()
        && i.samples >= ENTRY_WINDOW
        && f3 > h.enter_bps
        && f3 >= p.min_enter_funding_bps as i64
        // D8.1: never enter a position the exit rule would close next hour.
        && i.f_avg_bps >= h.exit_bps;
    match i.state {
        VaultState::Parked => {
            if !h.carry_ok {
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
            } else if h.carry_ok && !i.paused {
                Decision::ToParked
            } else {
                Decision::None
            }
        }
        VaultState::Basis => {
            if !i.market_open || i.samples == 0 {
                Decision::None
            } else if i.f_avg_bps < h.exit_bps {
                if h.carry_ok {
                    Decision::ToParked
                } else {
                    Decision::ToIdle
                }
            } else {
                Decision::None
            }
        }
        VaultState::Winding | VaultState::Unwinding | VaultState::SizingUp | VaultState::PartialUnwinding => Decision::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// D8 numbers: L = 39 %, r = 579 → be = 805; enter above 1005, exit below 705.
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
    const S_GOOD: u32 = 650;
    const S_BAD: u32 = 480;

    fn inputs(state: VaultState, f3: i64, f24: i64, s: u32, r: u32) -> Inputs {
        Inputs { state, f_avg_bps: f24, f_3h_bps: Some(f3), samples: 24, supply_bps: s, borrow_bps: r, market_open: true, paused: false }
    }

    #[test]
    fn break_even_from_decisions_d8() {
        let h = hurdles(&params(), S_GOOD, R);
        assert_eq!((h.be_bps, h.enter_bps, h.exit_bps), (805, 1005, 705));
        assert!(h.carry_ok);
        assert!(!hurdles(&params(), S_BAD, R).carry_ok);
    }

    #[test]
    fn carry_guard_wins_over_funding() {
        assert_eq!(evaluate(&params(), &inputs(VaultState::Parked, 3500, 3500, S_BAD, R)), Decision::ToIdle);
        assert_eq!(evaluate(&params(), &inputs(VaultState::Idle, 500, 500, S_BAD, R)), Decision::None);
        assert_eq!(evaluate(&params(), &inputs(VaultState::Idle, 500, 500, S_GOOD, R)), Decision::ToParked);
    }

    #[test]
    fn enters_on_the_3h_average_and_exits_on_the_24h_average() {
        let p = params();
        assert_eq!(evaluate(&p, &inputs(VaultState::Parked, 1005, 800, S_GOOD, R)), Decision::None);
        assert_eq!(evaluate(&p, &inputs(VaultState::Parked, 1006, 800, S_GOOD, R)), Decision::ToBasis);
        assert_eq!(evaluate(&p, &inputs(VaultState::Idle, 1006, 800, S_BAD, R)), Decision::ToBasis);
        // D8.1 (the live SPY case): a 3h spike with the 24h average under the exit line stays out.
        assert_eq!(evaluate(&p, &inputs(VaultState::Idle, 6698, -51, S_BAD, R)), Decision::None);
        assert_eq!(evaluate(&p, &inputs(VaultState::Parked, 1006, 704, S_GOOD, R)), Decision::None);
        assert_eq!(evaluate(&p, &inputs(VaultState::Parked, 1006, 705, S_GOOD, R)), Decision::ToBasis);
        // A high 24h average with a weak last three hours does not enter.
        assert_eq!(evaluate(&p, &inputs(VaultState::Parked, 900, 3500, S_GOOD, R)), Decision::None);
        // Exit: 24h below 705, whatever the 3h print says; Parked when carry allows, else Idle.
        assert_eq!(evaluate(&p, &inputs(VaultState::Basis, 5000, 705, S_GOOD, R)), Decision::None);
        assert_eq!(evaluate(&p, &inputs(VaultState::Basis, 5000, 704, S_GOOD, R)), Decision::ToParked);
        assert_eq!(evaluate(&p, &inputs(VaultState::Basis, 5000, 704, S_BAD, R)), Decision::ToIdle);
        assert_eq!(evaluate(&p, &inputs(VaultState::Basis, 0, 900, S_GOOD, R)), Decision::None);
    }

    #[test]
    fn floor_samples_market_and_pause_gates() {
        let mut p = params();
        p.enter_margin_bps = 0;
        let mut i = inputs(VaultState::Parked, 449, 449, 100, 40); // be 56, carry ok
        assert_eq!(evaluate(&p, &i), Decision::None);
        i.f_3h_bps = Some(450);
        assert_eq!(evaluate(&p, &i), Decision::ToBasis);
        i.samples = 2;
        i.f_3h_bps = None;
        assert_eq!(evaluate(&p, &i), Decision::None);
        i.samples = 3;
        i.f_3h_bps = Some(450);
        i.market_open = false;
        assert_eq!(evaluate(&p, &i), Decision::None);
        i.market_open = true;
        i.paused = true;
        assert_eq!(evaluate(&p, &i), Decision::None);
        let mut idle = inputs(VaultState::Idle, 0, 0, 650, 590);
        idle.paused = true;
        assert_eq!(evaluate(&p, &idle), Decision::None);
    }

    #[test]
    fn transitional_states_decide_nothing() {
        let p = params();
        for st in [VaultState::Winding, VaultState::Unwinding, VaultState::SizingUp, VaultState::PartialUnwinding] {
            assert_eq!(evaluate(&p, &inputs(st, 9000, 9000, S_GOOD, R)), Decision::None);
        }
        let mut closed = inputs(VaultState::Basis, 0, 0, S_BAD, R);
        closed.market_open = false;
        assert_eq!(evaluate(&p, &closed), Decision::None);
    }
}
