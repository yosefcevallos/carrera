# Decisions on top of spec v0.4.1

Date: 24 Sep 2026. These override `docs/handover/01-one-pager-and-spec-v0.4.md` where they conflict.

## D1. Parked mode is Kamino USDC supply, not ONyc

ONyc is deferred. While a vault is not in Basis, the borrowed USDC is supplied to
Kamino's USDC reserve and earns the supply APY `s`.

The negative-carry guard stays: if `s < r + carry_guard_margin` (borrow rate above
supply rate) the vault repays the loan and sits in **Idle** until either funding
clears the hurdle or supply rises back above borrow. At the rates recorded in the
spec (supply 4.8%, borrow 5.9%) the guard fires immediately, so the effective
default today is Idle. The UI must not describe Parked as "earning 4.8%" unless the
live rates say so.

Every ONyc-specific piece of v0.4.1 is dropped for now: `record_onyc_nav`, the NAV
ring buffer, ONyc trade bounds (§7.3), `onyc_cap`, `parked_apy_override`,
`max_parked_apy_bps`. `rebalance_from_parked` becomes a Kamino withdraw-and-repay.

## D2. Allocation rule with Kamino supply as the parked yield

```
cost_apy   = roundtrip_cost_bps × 8760 / expected_hold_hours
parked_apy = s                                   // Kamino USDC supply APY
hurdle_from_parked = parked_apy + L·r + cost_apy // primary loan interest cancels
hurdle_from_idle   = r          + L·r + cost_apy // no loan in Idle, Basis pays all of r

to BASIS  : (Parked && f_avg > hurdle_from_parked + enter_margin
          || Idle   && f_avg > hurdle_from_idle   + enter_margin)
          && f_avg >= min_enter_funding_bps && market_open && samples >= W
to PARKED : Basis  && f_avg < hurdle_from_parked − exit_margin && market_open && s >= r + carry_guard_margin
to IDLE   : Basis  && f_avg < hurdle_from_idle   − exit_margin && market_open && s <  r + carry_guard_margin
to IDLE   : Parked && s < r + carry_guard_margin
to PARKED : Idle   && s >= r + carry_guard_margin && !paused
```

All rates are annualised bps on basis notional `D`, as in v0.4.1.

## D3. The "4.5%" threshold

Product mentioned a 4.5% threshold for re-entering funding. As an annualised
funding rate that is below the borrow rate and would lose money, so it is not used
as the hurdle. It is kept as an absolute floor, `min_enter_funding_bps = 450`, that
Basis entry must also clear. If 4.5% was meant as the depositor's net yield on
stock value, at 30% LTV that is about 23% annualised funding, which is close to
the v0.4.1 entry level (≈21%) and needs no change. **Confirm which was meant.**

## D4. Venue integrations are behind an adapter boundary

Jupiter, Kamino, Phoenix and Hawkeye CPIs depend on verification items Q1–Q5 in
the spec. The program routes every venue call through `venues::*`. With the
`mock-venues` feature the adapters update the vault's accounting from
keeper-supplied values so the state machine, rule and exits can be tested
end-to-end on localnet. Without the feature they return `VenueNotWired`. Wiring
the real CPIs is M2/M3 work and does not change any instruction signature.

## D5. Frontend follows 03-FRONTEND-RECONCILIATION.md

Launch scope is the reference design's single page plus modal (allowed as the
canary UI) with these changes: mode labels are "Funding" and "Parked" (or "Idle");
no projected "you'd earn" numbers anywhere, only trailing realised share-price
growth; the live hurdle and enter/exit bands are shown as facts; stock-only
deposits; redemption preview handles the zero-USDC early-life case.
