# `program/` — Anchor program `carrera_overlay`

On-chain vaults for the xStock funding overlay. Interface: `../docs/CONTRACT.md`.
Rules: `../docs/DECISIONS.md` (Parked = Kamino USDC supply, hurdle rule D2, mock boundary D4).

Program id (localnet): `GH45ANLzg1t6rNnaoNqE39rN1rKGXFQXPZnNxvbhUmYw`

## Build and test

The active Solana CLI on this machine (2.2.6, platform-tools v1.45 / rustc 1.79) cannot
compile Anchor 0.32's dependency tree (edition 2024 crates). Use the installed 3.1.1
release (platform-tools v1.52):

```sh
export PATH=~/.local/share/solana/install/releases/3.1.1/solana-release/bin:$PATH

cargo test -p carrera_overlay --features mock-venues   # pure-logic unit tests (rule, nav, ring)
anchor build -- --features mock-venues                  # localnet build with simulated venues
anchor test --skip-build                                # mocha integration suite against a local validator
anchor build                                            # production build: venue calls return VenueNotWired
```

## Layout

| Path | What |
|---|---|
| `src/lib.rs` | `#[program]` entry, one thin fn per instruction |
| `src/state.rs` | `Registry`, `OverlayVault`, `VaultParams`, `ExitRequest`, `ExitEpoch`, enums |
| `src/rule.rs` | Allocation rule (D2): hurdles, hysteresis, carry guard, 450 bps floor. Pure, unit-tested |
| `src/nav.rs` | NAV, share price, pro-rata redemption incl. the negative-USDC case, fee shares. Pure, unit-tested |
| `src/ring.rs` | 24-sample funding ring buffer. Pure, unit-tested |
| `src/instructions/admin.rs` | `init_registry`, `set_roles`, `pause`/`unpause`, `init_vault`, `set_params`, `set_market_open` |
| `src/instructions/user.rs` | `deposit`, `request_exit`, `cancel_exit`, `redeem` |
| `src/instructions/oracle.rs` | `record_funding`, `record_kamino_rates`, `refresh_nav`, `mock_accrue` |
| `src/instructions/engine.rs` | `park`, `repay`, `wind_*`, `unwind_*`, `unwind_partial`, `size_up`, `rebalance_*` |
| `src/instructions/epoch.rs` | `close_epoch`, `settle_epoch`, `crystallise_fee` |
| `src/venues/` | The CPI boundary (below) |
| `tests/carrera_overlay.ts` | Integration suite: init → deposit → guard → wind to Basis → exit epoch → redeem |

## Feature flags

| Feature | Effect |
|---|---|
| `mock-venues` | Venue adapters simulate legs from the cached oracle price; `record_funding` / `record_kamino_rates` / `refresh_nav` accept their `mock_*` args; `mock_accrue` is enabled; the ≥59-min spacing on `record_funding` is lifted. **Never deploy with this feature.** |
| (none) | Every venue call returns `VenueNotWired`; `mock_*` args must be `None` or the call fails with `MockNotAllowed`. |
| `no-entrypoint`, `cpi`, `idl-build` | Standard Anchor features |

## Units

- Token amounts: base units. Shares have the xStock's decimals (`OverlayVault.stock_decimals`).
- `price_e6`: USD × 1e6 per whole xStock. `nav_usd_e6`: USDC base units (6 dp).
- `share_price_stock_e6`: 1_000_000 = one xStock per share (genesis).
- Rates: annualised bps (`borrow_apy_bps`, `supply_apy_bps`, `f_avg_bps`, hurdle).
- Funding samples (`OverlayVault.funding`, `record_funding` arg): **hourly** rate in bps × 1e6.
  35% annualised ⇒ `3500 × 1e6 / 8760 = 399_543` per hour. `f_avg_bps = mean × 8760 / 1e6`.

## Venue legs

Every external leg goes through `src/venues/*`. With `mock-venues` the adapters only update the
vault's accounting; without it they CPI into the venues using the account blocks the keeper passes in
`remaining_accounts` and the Borsh `VenueData` in each crank's trailing `venue_data` argument
(layouts in `docs/CONTRACT.md` "Venue blocks", and at the top of each adapter file).

| Venue | File | Wired | How |
|---|---|---|---|
| Kamino Lend | `venues/kamino.rs` | CPI, raw instructions | `refresh_reserve` ×2 + `refresh_obligation` before every mutation; `deposit_reserve_liquidity_and_obligation_collateral_v2`, `withdraw_obligation_collateral_and_redeem_reserve_collateral_v2` (collateral amount from the reserve exchange rate, rounded up, delivery checked on the custody balance), `borrow_obligation_liquidity_v2`, `repay_obligation_liquidity_v2` (partial, or `u64::MAX` to clear the loan: Kamino refuses to leave a sub-minimum residual, so `repay` clears the loan whenever the buffer plus the dust tolerance covers it, and the keeper keeps a small USDC cushion in `usdc_buffer` for accrued interest and cToken rounding), `deposit_reserve_liquidity` / `redeem_reserve_collateral` for the Parked USDC supply, `init_user_metadata` + `init_obligation` (keeper pays). Discriminators are `sha256("global:<name>")[..8]` (unit-tested). Farms placeholders are the program id because neither xStocks reserve nor the USDC reserve has farms. |
| Kamino reads | `venues/kamino.rs` | on-chain | `record_kamino_rates` derives borrow APR from the reserve's utilisation and rate curve and supply APR as `borrow × utilisation × (1 − take rate)`; `refresh_nav` reads `liquidity.market_price_sf` (≤ 10 min old) after an optional `refresh_reserve`. Offsets are the klend v1.25 sequential layout, validated against the live USDC and TSLAx reserves (`tests/fixtures/kamino/`, `tests/fixtures/kamino_layout.py`, unit test `reserve_fixture_parses`). |
| Jupiter v6 | `venues/jupiter.rs` | CPI, pass-through | The keeper fetches `swap-instructions` with the vault as user; the program checks program id, authority, mints and the vault's PDA token accounts, patches the amounts (its own oracle floor, slippage 0) and measures the destination balance delta. |
| Phoenix / Ember | `venues/phoenix.rs` | CPI via `phoenix-rise-ix` 0.6.5 builders | Ember deposit/withdraw, `deposit_funds` / `withdraw_funds`, `place_market_order` (short = Ask; close = reduce-only Bid), all-or-nothing IOC. Trader registration is done off-chain by the keeper (`register_trader` needs no trader signature). |
| Funding, margin | `venues/hawkeye.rs`, `phoenix.rs::read_equity` | keeper-supplied (DECISIONS D6) | `record_funding` accepts only a registered keeper's value on every build; Phoenix equity comes from `VenueData::phoenix_equity_usdc`. A future option is direct reads of the Phoenix `PerpAssetMap` (funding accumulator) and `Trader` accounts through the `phoenix-rise-accounts` layouts, which would remove the keeper from the rule's inputs. |

User `deposit` moves stock into custody only; the keeper's `sync_collateral` crank deposits custody into
the Kamino obligation, so user transactions never carry venue accounts.

**Verified here**: both builds compile for SBF; unit tests cover discriminators, the reserve layout on
live fixtures, rate-curve maths, Jupiter payload patching and the Phoenix builders; the mock localnet
suite (7 tests) covers the state machine with the new argument shapes. **Fork-tested (LiteSVM, real
klend + Farms programs and mainnet accounts, `fork-tests/`)**: the whole Kamino loop on the plain build
against the live TSLAx and USDC reserves — `init_kamino_obligation` (user metadata, obligation, debt-farm
user state), `sync_collateral` (collateral deposit), on-chain rates and price reads, `wind_start` from Idle
(borrow of 113.30 USDC at 30% LTV against 1 TSLAx, with the reserve's debt farm), `unwind_commit`
(USDC supply), guardian `repay` (redeem + repay-all, interest paid from the cushion), `settle_epoch`
(collateral withdraw) and `redeem` returning the full TSLAx. **Not fork-tested**: Jupiter (routes are
slot-bound) and Phoenix (needs its live trader-index buffers); those two are encoding-tested only.

Run the fork test with `cd fork-tests && cargo test` (its `rust-toolchain.toml` pins Rust 1.97.1, which
litesvm 0.16's Agave 4.2 crates need; the Anchor build keeps using the default toolchain). Fixtures live in
`tests/fixtures/kamino/` and are refreshed with `tests/fixtures/dump.py` (dump everything in one call, the
reserves and their token vaults must be consistent); `tests/fixtures/kamino_clock.py` prints the Scope
timestamps the test clock must start after, and `kamino_layout.py` documents the reserve offsets.

**Still open (spec §10)**: Phoenix maintenance margin per market (Q2) → `min_margin`/tier `L` for C/D;
Kamino reserve deposit/borrow caps (Q3); Jupiter depth → `deposit_cap`/`basis_cap` (Q5); measured
round-trip cost (Q6); Kamino interest accrual on `debt_*` (NAV uses the cached debt); v0 transactions
with Jupiter lookup tables in the keeper sender; Phoenix trader-index account resolution in the keeper.

## Deviations from CONTRACT.md

1. `OverlayVault.stock_decimals: u8` added (before `bump`). Needed for USD math; set from the xStock mint at `init_vault`.
2. Funding unit: `record_funding(mock_rate_bps_hourly)` and the ring are hourly bps × 1e6, not whole bps (whole bps per hour is too coarse).
3. `record_funding`, `record_kamino_rates`, `refresh_nav` take a `signer` as account 0 (any key) so keeper encoding is uniform.
4. `close_epoch` needs `system_program` appended (it creates the epoch account when nobody requested an exit).
5. `redeem` accounts follow the contract (`share_mint` + `escrow_shares` present) and it burns the escrowed shares; `settle_epoch` only removes them from `total_shares`.
6. `wind_start` is valid from Idle as well as Parked; from Idle it borrows `D` first (D2's idle hurdle). Spec §5 listed Parked only.
7. `rebalance_from_parked` is also allowed while exits are pending (spec §8's de-lever step), not only above `L + band`.
8. `mock_accrue(usdc, leg)` exists in mock builds only (simulates earned USDC for tests).
9. Exit fee is applied when `settle_epoch` draws on live Phoenix collateral (vault in Basis); `emergency_margin` = `min_margin_bps`.
10. `DEBT_DUST_USDC = 10_000` is a program constant, not a `VaultParams` field (adding a field would change the live vault layout). Debt below it never blocks `settle_epoch`, is written off by `repay`/`unwind_commit`/`rebalance_from_parked`, and is reported in `NavRefreshed.debt_dust_usdc`. `repay` is valid from Idle too and draws `usdc_buffer` before supplied USDC.
11. Engine cranks, `settle_epoch` and the new `sync_collateral` / `init_kamino_obligation` take a trailing `venue_data: Vec<u8>` (empty on mock builds); venue accounts travel as remaining-account blocks.
12. Token programs: xStock mints are Token-2022 on mainnet, so stock mints and stock token accounts use `anchor_spl::token_interface` and every stock move is `transfer_checked` through a separate `stock_token_program` account (appended to `init_vault`, `deposit`, `redeem`, `settle_epoch`; `xstock_mint` also appended to `redeem` and `settle_epoch`). Shares and USDC stay classic SPL Token. The localnet test creates the stock mint with the live mints' transfer-relevant extensions (transfer hook with no program, permanent delegate, pausable).
    Implications of the live extensions: **permanent delegate** means the issuer (Backed) can move or burn xStock out of any account, including the vault custody PDAs; **pausable** means the issuer can halt all transfers, which would block `deposit`, `settle_epoch` and `redeem` until unpaused; the **transfer hook** extension exists but no hook program is set, so no extra accounts are needed today. If a hook program is ever set, `deposit`/`settle_epoch`/`redeem` must resolve its extra account metas via `remaining_accounts`.

## Not done here

- Fork tests for Jupiter (slot-bound routes) and Phoenix (live trader-index buffers); both are encoding-tested only.
- Kamino interest accrual on `debt_*` is not modelled; NAV uses the cached debt.
- Address lookup tables / v0 transactions in the keeper sender (required for Jupiter routes).
- Property tests over price paths (§14).
