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

## D6. Keeper stays the Phoenix oracle for now; no Hawkeye

Decided 24 Sep 2026. `record_funding` and the margin read used by rebalancing take
keeper-supplied values (the keeper fetches Phoenix's public API and Kamino's API and
pushes them on-chain; only registered keepers may do so). Hawkeye is not used.

**Future engineering option, not chosen yet:** read Phoenix state directly in the
program by deserialising the market and trader/subaccount accounts with Phoenix's own
layouts from their SDK crate (`phoenix-rise`). No Hawkeye CPI, same trust properties
as Hawkeye (the program reads chain state itself instead of trusting the keeper), at
the cost of slightly more code to maintain whenever Phoenix changes account layouts.
Revisit when the venue legs go live and the keeper's oracle role becomes the main
trust assumption.

## D7. Per-vault LTV from a uniform 40% drop buffer (replaces spec §6.1 tiers)

Decided 24 Sep 2026, product sign-off in chat. The spec's four flat tiers ignored the
real venue parameters. Read on chain from the Kamino xStocks reserves and from
Phoenix's market list on 24 Sep 2026:

| Vault | Kamino max LTV | Kamino liq LTV | Phoenix max lev | Maintenance (50% of initial) |
|---|---|---|---|---|
| SPY | 73% | 75% | 20x | 2.5% |
| QQQ | 70% | 72% | 20x | 2.5% |
| GOOGL | 60% | 70% | 20x | 2.5% |
| TSLA | 55% | 65% | 20x | 2.5% |
| NVDA | 55% | 65% | 20x | 2.5% |
| AAPL | 40% | 50% | 20x | 2.5% |
| MSTR | 30% | 40% | 10x | 5% |
| CRCL | 30% | 40% | 10x | 5% |
| HOOD | 30% | 40% | 10x | 5% |

**Why a drop buffer is the right knob.** With the overlay structure the vault's Kamino
LTV equals `L` by construction (the loan buys stock that is posted as collateral), so:

- stock drop before Kamino liquidates: `1 − L / liqLTV`
- stock rise before Phoenix liquidates, pre-rebalance: `L − maintenance`
- depositor yield on stock value: `L·f − L(1+L)·r`

Raising `L` improves both the yield and the Phoenix buffer; it only costs Kamino
downside. So we fix the downside distance and let `L` fall out per market.

**Rule:** `L = 0.6 × liqLTV`, rounded to whole percent, capped at Kamino's max LTV.
That gives every vault a 40% drop buffer and ≥ 19% rise buffer before any rebalance.
40% covers every single-day move on record for these large caps; the small caps
(MSTR, CRCL, HOOD) are held at 24% because Kamino's 40% liquidation line, not our
appetite, is the binding constraint, and those names carry crypto-beta gap risk.

| Vault | L (ltv_bps) | liq_ltv_bps | emergency_ltv_bps (liq − 500) | min_margin_bps |
|---|---|---|---|---|
| SPY | 4500 | 7500 | 7000 | 1000 |
| QQQ | 4300 | 7200 | 6700 | 1000 |
| GOOGL | 4200 | 7000 | 6500 | 1000 |
| TSLA | 3900 | 6500 | 6000 | 1000 |
| NVDA | 3900 | 6500 | 6000 | 1000 |
| AAPL | 3000 | 5000 | 4500 | 1000 |
| MSTR | 2400 | 4000 | 3500 | 1200 |
| CRCL | 2400 | 4000 | 3500 | 1200 |
| HOOD | 2400 | 4000 | 3500 | 1200 |

`min_margin_bps` is the Phoenix margin floor the keeper defends: 4× maintenance on
20x markets (10%), and 12% on 10x markets so the rebalance band (+5%) sits under the
24% initial margin. Alternatives considered: a 50% buffer (`0.5 × liq`) reproduces
the spec's numbers with 25–40% less yield; a 33% buffer (`0.67 × liq`) adds one to
two points of yield for materially less gap protection. Re-run
`deploy/set-tiers.ts` whenever Kamino changes a reserve's LTVs; the keeper reads the
vault params from chain and needs no change.

## D8. Enter fast on a short window, exit slow on the 24h average (replaces D2's entry rule)

Decided 25 Sep 2026 after the first real wind. Funding regimes on these markets last days,
so waiting for a full 24-sample window before entering leaves yield on the table, while a
one-hour print is too noisy to trade on (each false entry costs a round trip, measured at
18 bps of basis notional in the fork). The vault pays Kamino on both loans, so the funding
it needs to break even is `be = r + L·r` (≈ 8% at today's rates).

```
f_3h  = mean of the last 3 hourly funding samples, annualised
f_24h = mean of the last 24 hourly funding samples, annualised (as before)

to BASIS  : Idle/Parked && f_3h  > be + enter_margin (200 bps) && f_3h >= min_enter_funding (450)
            && samples >= 3 && market_open && !paused
to IDLE   : Basis && f_24h < be − exit_margin (100 bps)   (Parked instead when the carry guard allows)
```

Cost amortisation drops out of the entry rule: a short expected hold is the point. The
4% figure discussed is below break-even and is not used as an entry level; the 450 bps
floor stays as a sanity bound. `RuleEvaluated` reports both averages and `be`.
