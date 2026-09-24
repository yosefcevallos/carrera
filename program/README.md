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

## What is mocked and where the real CPI goes

| Venue | File | Functions to wire (spec ref) |
|---|---|---|
| Jupiter v6 | `src/venues/jupiter.rs` | `swap_usdc_to_stock`, `swap_stock_to_usdc` — `shared_accounts_route` with out-mint check and `min_out` from the Kamino oracle (§7.4) |
| Kamino Lend | `src/venues/kamino.rs` | `deposit_collateral`, `withdraw_collateral`, `borrow_usdc`, `repay_usdc`, `supply_usdc`, `withdraw_supplied_usdc`, `read_rates` (USDC reserve), `read_price` (xStock reserve oracle) |
| Phoenix / Ember | `src/venues/phoenix.rs` | `deposit_collateral`, `withdraw_collateral` (Ember wrap/unwrap + subaccount), `open_short`, `close_short` (bounded, `last_valid_slot`), `read_equity` (Hawkeye `view_margin`) |
| Hawkeye | `src/venues/hawkeye.rs` | `read_funding` (`view_funding` return data) |

Instruction handlers own the accounting (`collateral_qty`, `basis_spot_qty`, `debt_*`,
`parked_usdc`, `phoenix_*`); adapters only execute or read. Wiring the CPIs changes no
instruction signature. Real wiring will also need the venue accounts added to the
`KeeperVault` context (or passed as remaining accounts) and address lookup tables.

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
10. Token program: classic SPL Token (`anchor_spl::token`). If xStocks turn out to be Token-2022, switch to `token_interface` (mechanical).

## Not done here

- Real venue CPIs (M2/M3), address lookup tables, LiteSVM fixtures, property tests over price paths (§14).
- Kamino interest accrual on `debt_*` is not modelled; NAV uses the cached debt until the real Kamino read replaces it.
