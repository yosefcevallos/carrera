//! Mirror of the on-chain allocation rule (docs/DECISIONS.md, D2). The keeper uses it
//! only to decide which crank to send; the program re-evaluates in the same
//! transaction and rejects anything the rule does not permit.

use crate::accounts::{VaultParams, VaultState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    None,
    ToBasis,
    ToParked,
    ToIdle,
}

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub struct Hurdles {
    pub cost_apy_bps: i64,
    pub l_r_bps: i64,
    /// Basis must beat Kamino supply plus the secondary loan's interest plus cost.
    pub from_parked_bps: i64,
    /// From Idle there is no loan, so Basis must also cover the primary loan's interest.
    pub from_idle_bps: i64,
    /// Kamino supply covers borrow plus the carry-guard margin.
    pub carry_ok: bool,
}

pub fn hurdles(p: &VaultParams, supply_bps: u32, borrow_bps: u32) -> Hurdles {
    let hold = p.expected_hold_hours.max(1) as i64;
    let cost_apy_bps = p.roundtrip_cost_bps as i64 * 8760 / hold;
    let l_r_bps = p.ltv_bps as i64 * borrow_bps as i64 / 10_000;
    Hurdles {
        cost_apy_bps,
        l_r_bps,
        from_parked_bps: supply_bps as i64 + l_r_bps + cost_apy_bps,
        from_idle_bps: borrow_bps as i64 + l_r_bps + cost_apy_bps,
        carry_ok: supply_bps as i64 >= borrow_bps as i64 + p.carry_guard_margin_bps as i64,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Inputs {
    pub state: VaultState,
    pub f_avg_bps: i64,
    pub samples: u8,
    pub supply_bps: u32,
    pub borrow_bps: u32,
    pub market_open: bool,
    pub paused: bool,
}

pub fn evaluate(p: &VaultParams, i: &Inputs) -> Decision {
    let h = hurdles(p, i.supply_bps, i.borrow_bps);
    let enter = p.enter_margin_bps as i64;
    let exit = p.exit_margin_bps as i64;
    let enough_samples = i.samples >= p.funding_window;
    let floor_ok = i.f_avg_bps >= p.min_enter_funding_bps as i64;
    match i.state {
        VaultState::Parked => {
            if i.market_open && enough_samples && floor_ok && i.f_avg_bps > h.from_parked_bps + enter {
                Decision::ToBasis
            } else if !h.carry_ok {
                Decision::ToIdle
            } else {
                Decision::None
            }
        }
        VaultState::Idle => {
            if i.paused {
                Decision::None
            } else if i.market_open && enough_samples && floor_ok && i.f_avg_bps > h.from_idle_bps + enter {
                Decision::ToBasis
            } else if h.carry_ok {
                Decision::ToParked
            } else {
                Decision::None
            }
        }
        VaultState::Basis => {
            if !i.market_open {
                Decision::None
            } else if h.carry_ok && i.f_avg_bps < h.from_parked_bps - exit {
                Decision::ToParked
            } else if !h.carry_ok && i.f_avg_bps < h.from_idle_bps - exit {
                Decision::ToIdle
            } else {
                Decision::None
            }
        }
        VaultState::Winding | VaultState::Unwinding => Decision::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn inputs(state: VaultState, f: i64, s: u32, r: u32) -> Inputs {
        Inputs { state, f_avg_bps: f, samples: 24, supply_bps: s, borrow_bps: r, market_open: true, paused: false }
    }

    #[test]
    fn hurdle_numbers_from_decisions_d2() {
        let h = hurdles(&params(), 650, 590);
        assert_eq!(h.cost_apy_bps, 730);
        assert_eq!(h.l_r_bps, 177);
        assert_eq!(h.from_parked_bps, 1557);
        assert_eq!(h.from_idle_bps, 590 + 177 + 730);
        assert!(h.carry_ok);
        assert!(!hurdles(&params(), 480, 590).carry_ok);
    }

    #[test]
    fn spec_rates_fire_the_carry_guard() {
        // supply 4.8% < borrow 5.9% + 50 bps → Parked repays to Idle, Idle stays Idle.
        assert_eq!(evaluate(&params(), &inputs(VaultState::Parked, 1000, 480, 590)), Decision::ToIdle);
        assert_eq!(evaluate(&params(), &inputs(VaultState::Idle, 1000, 480, 590)), Decision::None);
        // Once supply clears borrow, Idle parks.
        assert_eq!(evaluate(&params(), &inputs(VaultState::Idle, 1000, 650, 590)), Decision::ToParked);
    }

    #[test]
    fn hysteresis_around_parked_hurdle() {
        let p = params();
        // hurdle 1557: enter above 1757, exit below 1457.
        assert_eq!(evaluate(&p, &inputs(VaultState::Parked, 1757, 650, 590)), Decision::None);
        assert_eq!(evaluate(&p, &inputs(VaultState::Parked, 1758, 650, 590)), Decision::ToBasis);
        assert_eq!(evaluate(&p, &inputs(VaultState::Basis, 1457, 650, 590)), Decision::None);
        assert_eq!(evaluate(&p, &inputs(VaultState::Basis, 1456, 650, 590)), Decision::ToParked);
        // In the band nothing moves.
        assert_eq!(evaluate(&p, &inputs(VaultState::Basis, 1600, 650, 590)), Decision::None);
        assert_eq!(evaluate(&p, &inputs(VaultState::Parked, 1600, 650, 590)), Decision::None);
    }

    #[test]
    fn idle_variant_pays_full_primary_interest() {
        let p = params();
        // hurdle_from_idle 1497 → enter above 1697.
        assert_eq!(evaluate(&p, &inputs(VaultState::Idle, 1697, 480, 590)), Decision::None);
        assert_eq!(evaluate(&p, &inputs(VaultState::Idle, 1698, 480, 590)), Decision::ToBasis);
        // Basis with negative carry exits to Idle below 1397, not to Parked.
        assert_eq!(evaluate(&p, &inputs(VaultState::Basis, 1396, 480, 590)), Decision::ToIdle);
        assert_eq!(evaluate(&p, &inputs(VaultState::Basis, 1397, 480, 590)), Decision::None);
    }

    #[test]
    fn floor_samples_market_and_pause_gates() {
        let mut p = params();
        // Make the hurdle trivially low so only the 450 bps floor binds.
        p.roundtrip_cost_bps = 0;
        p.enter_margin_bps = 0;
        let mut i = inputs(VaultState::Parked, 449, 0, 0);
        assert_eq!(evaluate(&p, &i), Decision::ToIdle); // floor blocks; supply 0 < borrow+50
        i.f_avg_bps = 450;
        assert_eq!(evaluate(&p, &i), Decision::ToBasis);
        i.samples = 23;
        assert_eq!(evaluate(&p, &i), Decision::ToIdle);
        i.samples = 24;
        i.market_open = false;
        assert_eq!(evaluate(&p, &i), Decision::ToIdle);
        let mut idle = inputs(VaultState::Idle, 0, 650, 590);
        idle.paused = true;
        assert_eq!(evaluate(&p, &idle), Decision::None);
        assert_eq!(evaluate(&p, &inputs(VaultState::Winding, 9999, 650, 590)), Decision::None);
        assert_eq!(evaluate(&p, &inputs(VaultState::Basis, 0, 650, 590)), Decision::ToParked);
        let mut closed = inputs(VaultState::Basis, 0, 650, 590);
        closed.market_open = false;
        assert_eq!(evaluate(&p, &closed), Decision::None);
    }
}
