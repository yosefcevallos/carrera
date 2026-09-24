# carrera-keeper

Off-chain crank for the `carrera_overlay` program (spec §12). It records inputs,
evaluates a mirror of the allocation rule to pick which crank to send, moves loans
between Parked and Basis while the market is open, processes exit epochs,
rebalances every minute, raises alerts, and serves a status API for the ops
dashboard. It holds no funds beyond fee SOL and can never withdraw: the program
re-checks every rule and bound in the same transaction and rejects anything else.

It does **not** depend on the program crate. Instructions are encoded from
`docs/CONTRACT.md` (Anchor discriminator `sha256("global:<name>")[..8]` plus Borsh
args) and accounts are decoded by field order. If the contract changes, `ix.rs`
and `accounts.rs` are the only files to touch.

## Run

```sh
cp keeper.example.toml keeper.toml   # edit rpc_url, keypair_path, program_id, mints
cargo run -- status                  # print every vault: state, f_avg, hurdle, LTV, margin, NAV
cargo run -- once hourly             # one hourly pass and exit
cargo run -- once fast               # one 60 s pass and exit
cargo run -- run                     # both loops under the leader lease, plus the status server
cargo run -- run --mock mock.example.json   # feed inputs from a file (program built with mock-venues)
```

`--config PATH` or `CARRERA_KEEPER_CONFIG` selects the config file (default
`keeper.toml`). Env overrides: `CARRERA_RPC_URL`, `CARRERA_KEYPAIR`,
`CARRERA_PROGRAM_ID`, `CARRERA_ALERT_WEBHOOK`. Log level via `RUST_LOG`.

```sh
cargo build && cargo test && cargo clippy
```

## Config keys

| Key | Default | Meaning |
|---|---|---|
| `rpc_url` | – | Solana RPC endpoint |
| `keypair_path` | – | Keeper hot key (must be in the registry's keeper set) |
| `program_id` | – | `carrera_overlay` program id |
| `usdc_mint` | – | USDC mint |
| `kamino_reserve` | none | Kamino USDC reserve passed to `record_kamino_rates` (live mode) |
| `treasury_shares` | none | Share token account for performance fees; `crystallise_fee` is skipped when unset |
| `hourly_interval_secs` | 3600 | Hourly loop period |
| `fast_interval_secs` | 60 | Rebalance loop period |
| `alert_webhook_url` | none | POST `{"text": ...}` on each alert |
| `min_keeper_sol` | 0.5 | Alert when the keeper's balance drops below this |
| `lease_path` | `/tmp/carrera-keeper.lease` | Leader lease file |
| `lease_ttl_secs` | 90 | Lease expiry; renewed every ttl/3 |
| `status_bind` | `127.0.0.1:8787` | HTTP status server bind address |
| `history_path` | none | JSONL file that every fast-loop history sample is appended to |
| `vaults.<SYMBOL>.mint` | – | xStock mint |
| `vaults.<SYMBOL>.stock_decimals` | 8 | Used for the keeper's own LTV / margin / notional estimates |
| `vaults.<SYMBOL>.hawkeye_view` | none | Hawkeye view account for `record_funding` (live mode) |
| `vaults.<SYMBOL>.oracle` | none | Price account for `refresh_nav` (live mode) |

## What each loop does

**Hourly** (`hourly.rs`), per spec §12: `record_funding` for every vault →
`record_kamino_rates` → per vault: `refresh_nav`, `set_market_open` if the NYSE
calendar flag changed, evaluate the rule mirror, send the transition (`wind_start`
+ `wind_step 1..3` + `wind_commit`, `unwind_start` + steps + `unwind_commit`, `park`,
`repay`; a vault found mid-Winding or mid-Unwinding is resumed from its `step`),
`size_up` when LTV is below `L − size_band`, settle any closed-but-unsettled
previous epoch, `close_epoch` + `settle_epoch` when the current epoch's window has
elapsed and holds shares, `crystallise_fee` when share price is above the high-water
mark, final `refresh_nav`. Every send is best-effort: failures are logged and the
pass continues, because the program is the authority on what is allowed.

**Fast** (`fast.rs`), every 60 s per §6.3: read the vault, update the status book
and alerts, then in Basis `rebalance_to_kamino` when LTV > `L + 800 bps` or
`rebalance_to_phoenix` when margin < `min_margin + 500 bps`; in Parked
`rebalance_from_parked(amount)` when LTV > `L + 800 bps` with `amount` the USDC
that brings LTV back to `L`; emergency first: `unwind_start(emergency)` in Basis
or `repay` in Parked when LTV > `emergency_ltv` or margin < `min_margin`.

## Rule mirror

`rule.rs` implements docs/DECISIONS.md D2 with Kamino supply as the parked yield:

```
cost_apy           = roundtrip_cost_bps × 8760 / expected_hold_hours
hurdle_from_parked = s + L·r + cost_apy
hurdle_from_idle   = r + L·r + cost_apy
```

Basis entry also requires `f_avg ≥ min_enter_funding_bps` (the 450 bps floor),
24 samples and an open market. The carry guard `s ≥ r + carry_guard_margin` decides
Parked vs Idle. The keeper only uses the mirror to choose a crank; the program
evaluates the same rule on-chain and refuses transitions it does not permit.

## Units

Funding samples on the vault are the **hourly** rate in **bps × 1e6** (1 bps/hour ==
`1_000_000`), so `f_avg_bps = mean(samples) × 8760 / 1e6`. 35% annualised funding is
`3500 / 8760 ≈ 0.3995 bps/hour ≈ 399_543`. All other rates are annualised bps;
prices and USD values carry 6 decimals (`_e6`).

## `--mock`

Passes `Some(value)` for the `mock_*` args of `record_funding`, `record_kamino_rates`
and `refresh_nav` from a JSON file (see `mock.example.json`: Kamino borrow/supply
bps, and per vault `funding_hourly_bps_e6` and `price_e6`). Only a program built
with the `mock-venues` feature accepts these; a production build rejects them with
`MockNotAllowed`. The file is re-read on every hourly pass so it can be edited
while the keeper runs. Without `--mock` every mock arg is `None` and the program
reads the Hawkeye, Kamino and oracle accounts named in the config.

## Leader lease

Two instances may run at once. `lease.rs` uses a small JSON file (`lease_path`)
holding `{holder, expires_unix}`, written atomically via temp file + rename, renewed
every `lease_ttl_secs / 3`. An instance runs the loops only while it holds the
lease; the standby takes over when the file expires or is released on Ctrl-C. Both
instances must see the same path (same host or shared volume). The lease is
advisory: if it were ever held twice, the program's state machine rejects duplicate
cranks, so the cost is a few failed transactions, never a wrong state. Swap this
module for a Postgres or Redis lease if the instances live on different hosts.

## Alerts

Each condition logs at WARN and POSTs to `alert_webhook_url` if set, at most once
per 15 minutes per key, and appears in `GET /status` under `keeper.alerts`:

| Key | Level | Condition |
|---|---|---|
| `<vault>:stuck` | crit | Winding or Unwinding for more than 15 minutes |
| `<vault>:ltv` | crit | LTV within 300 bps of `emergency_ltv` |
| `<vault>:margin` | crit | Phoenix margin within 300 bps of `min_margin` |
| `<vault>:nav` | warn | NAV cache older than `max_nav_age_slots` |
| `keeper:sol` | warn | Keeper balance below `min_keeper_sol` |

## Status API (`status.rs`)

Bound at `status_bind`, GET only, `Access-Control-Allow-Origin: *`. Numbers come
from the loops' own account reads; requests never hit RPC.

- `GET /status` → `{ keeper, vaults }`.
  `keeper`: `instance_id`, `is_leader`, `last_hourly_run_ts`, `last_fast_run_ts`,
  `hourly_ok`, `fast_ok`, `sol_balance`, `program_id`, `cluster`, `registry_paused`, `alerts[]`.
  Each vault is a book: `symbol`, `state` (`idle|parked|winding|basis|unwinding`),
  `step`, `market_open`, `opened_ts` (when the keeper first saw the current state),
  `tier`, `stock_decimals` (from the vault account, config fallback when 0), `usdc_decimals` (6),
  `spot_mark_e6`, `perp_mark_e6`, `idle_margin_usdc_e6`, `basis_at_open_bps`, `basis_estimated`,
  `legs[]`, `net_delta`, `carry`, `rule`, `ltv_bps`, `liq_ltv_bps`,
  `margin_bps`, `min_margin_bps`, `emergency_ltv_bps`, `nav_usd_e6`,
  `share_price_stock_e6`, `nav_slot`, `nav_age_slots`, `pending_exit_shares`, `epoch_id`.
- `GET /history?vault=TSLA&hours=168` → `[{ ts, state, f_avg_bps, hurdle_bps, ltv_bps, margin_bps, nav_usd_e6, share_price_stock_e6 }]`,
  one sample per fast tick, kept in memory for 14 days and appended to `history_path` as JSONL (`vault` field added).
- `GET /healthz` → 200 when the fast loop ran within the last 3 minutes, else 503.

**Legs.** One entry per non-zero position: `long_spot` (Kamino, `basis_spot_qty`,
rate 0), `borrow_usdc` (Kamino, `debt + debt_b`, rate `−r`), `supply_usdc` (Kamino,
`parked_usdc`, rate `+s`), `short_perp` (Phoenix, `phoenix_short_qty`, rate `+f_avg`).
`mark_e6` is the vault's cached price (1e6 for USDC legs).

**Liquidation estimates.**
- Kamino side (spot and borrow legs): LTV hits `liq_ltv` when the price falls to
  `liq_price = price × ltv / liq_ltv`; `liq_distance_bps = 10000 × (1 − ltv / liq_ltv)`.
- Phoenix short: `liq_price = mark × (1 + buffer)` with the tier buffer from spec
  §6.1 (A/B 26%, C 21%, D 16%); `liq_distance_bps = buffer`. This is a static tier
  figure, not a live margin-based number; replace it with each market's maintenance
  margin once Phoenix leverage tiers are confirmed (spec §10 Q2).

**Marks.** `spot_mark_e6` and `perp_mark_e6` are exposed separately so the UI never
has to guess, but until Hawkeye is wired both are the vault's cached oracle price.

**Basis-only fields.** `idle_margin_usdc_e6 = phoenix_equity − min_margin × short notional`
(negative when the subaccount is below `min_margin`). `basis_at_open_bps =
(perp_mark − spot_mark) / spot_mark` recorded on the tick the keeper first saw the
vault in Basis; `basis_estimated: true` says both marks came from the same cached
price. Both are `null` outside Basis.

**Net delta.** Basis legs only, depositor stock excluded:
`qty = basis_spot_qty − phoenix_short_qty`, `usd_e6 = qty × price / 10^decimals`.

**Carry** (`estimated: true`). Reset when the state changes; every fast tick adds
`(f_avg × short_notional + s × parked_usdc − r × (debt + debt_b)) × dt / year`.
`ann_net_bps = (f_avg in Basis | s in Parked | 0 in Idle) − r − L·r`.

## Files

| File | Purpose |
|---|---|
| `src/ix.rs` | Instruction builders, PDAs, discriminators (tested) |
| `src/accounts.rs` | Borsh decoders, LTV / margin / f_avg helpers (tested) |
| `src/rule.rs` | Allocation rule mirror (tested) |
| `src/calendar.rs` | NYSE session helper (tested) |
| `src/lease.rs` | File leader lease (tested) |
| `src/status.rs` | Status API, books, carry, history (tested) |
| `src/hourly.rs`, `src/fast.rs` | The two loops |
| `src/alerts.rs`, `src/venues.rs`, `src/chain.rs`, `src/config.rs`, `src/main.rs` | Alerts, input source, RPC, config, CLI |
