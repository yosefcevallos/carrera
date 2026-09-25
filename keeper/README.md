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
| `kamino_reserve` | none | Kamino USDC reserve passed to `record_kamino_rates` (onchain feed) |
| `feed` | `onchain` | Where hourly inputs come from: `onchain`, `live` or `mock` (see "Feeds") |
| `phoenix_api_url` | `https://perp-api.phoenix.trade` | Phoenix REST base (live feed) |
| `kamino_api_url` | `https://api.kamino.finance` | Kamino REST base (live feed) |
| `kamino_market` | `5wJeMr…ULsua` | Kamino lending market whose USDC reserve sets borrow/supply (the "xStocks Market") |
| `jupiter_price_url` | `https://lite-api.jup.ag/price/v3` | Jupiter price endpoint (live feed) |
| `mock_path` | none | JSON file for `feed = "mock"` |
| `treasury_shares` | none | Share token account for performance fees; `crystallise_fee` is skipped when unset |
| `program_build` | `mock` | `mock`: cranks carry no venue accounts. `real`: Kamino + Phoenix blocks, Jupiter routes and the per-vault lookup table are attached automatically (see "Venue blocks in the loops") |
| `hourly_interval_secs` | 3600 | Hourly loop period |
| `fast_interval_secs` | 60 | Rebalance loop period |
| `settle_interval_secs` | 300 | Settlement pass period (exit epochs) |
| `market_open` | `auto` | `auto`: NYSE calendar AND Phoenix state; `open`: force the on-chain flag true and never flip it false (bypasses spec §7.5, demo only); `closed`: force false |
| `alert_webhook_url` | none | POST `{"text": ...}` on each alert |
| `min_keeper_sol` | 0.5 | Alert when the keeper's balance drops below this |
| `lease_path` | `/tmp/carrera-keeper.lease` | Leader lease file |
| `lease_ttl_secs` | 90 | Lease expiry; renewed every ttl/3 |
| `status_bind` | `127.0.0.1:8787` | HTTP status server bind address |
| `history_path` | none | JSONL file that every fast-loop history sample is appended to |
| `vaults.<SYMBOL>.mint` | – | xStock mint |
| `vaults.<SYMBOL>.stock_decimals` | 8 | Used for the keeper's own LTV / margin / notional estimates |
| `vaults.<SYMBOL>.hawkeye_view` | none | Hawkeye view account for `record_funding` (onchain feed) |
| `vaults.<SYMBOL>.oracle` | none | Price account for `refresh_nav` (onchain feed) |
| `vaults.<SYMBOL>.phoenix_market` | the symbol | Phoenix perp symbol for the live feed (equity perps are bare tickers: `TSLA`) |
| `vaults.<SYMBOL>.stock_token_program` | Token-2022 | Token program owning the xStock mint; passed to `settle_epoch` |
| `vaults.<SYMBOL>.kamino_reserve` | none | The xStock's reserve in the Kamino xStocks market (informational) |

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

**Fast** (`fast.rs`), every 60 s per §6.3: in live mode, first reload Jupiter prices
if any are missing or older than 5 minutes (one request; funding, rates and market
state are left to the hourly reload); read the vault, update the status book
and alerts, then in Basis `rebalance_to_kamino` when LTV > `L + 800 bps` or
`rebalance_to_phoenix` when margin < `min_margin + 500 bps`; in Parked
`rebalance_from_parked(amount)` when LTV > `L + 800 bps` with `amount` the USDC
that brings LTV back to `L`; emergency first: `unwind_start(emergency)` in Basis
or `repay` in Parked when LTV > `emergency_ltv` or margin < `min_margin`.

**Settle** (`settle.rs`), every `settle_interval_secs`: for each vault with
`pending_exit_shares > 0` whose epoch window has elapsed, refresh NAV if older than a
third of `max_nav_age_slots`, then free the liability by state: Unwinding → resume
`unwind_step` from the current step and `unwind_commit`; Basis → `unwind_partial(
fraction, ExitDemand)` with `fraction_bps = pending_exit_shares / total_shares` when the
exit is smaller than the whole position, otherwise `unwind_start(ExitDemand)` + steps +
commit; Parked → `repay`; Idle with residual `debt_usdc` → `repay` (the program accepts
repay from Idle after the hotfix). Then `close_epoch` and `settle_epoch`. Before that, on
every pass, it reads the `ExitEpoch` for `epoch_id − 1` and, if it is closed but not
settled, releases and settles it regardless of timing (a close without a settle must
not wait on the next epoch's window). The hotfixed program ignores debt dust ≤ 10_000
USDC at settlement, so an Idle vault with dust is not repaid first. One log line per
vault per pass. `carrera-keeper once settle` runs a single pass.

**Market-open flag.** With `market_open = "auto"` the hourly pass writes the stricter
of the NYSE cash session and Phoenix's market state (spec §7.5). With `"open"` it
writes `true` once and never flips it to `false`, so exit-driven unwinds can settle
outside cash hours. That bypasses §7.5 and is for the demo only.

**Stale-preflight retry.** Every send retries up to two more times with a fresh
blockhash when preflight reports `Blockhash not found` or block height exceeded; any
other error is returned as is.

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

## Feeds (`feed.rs`, `venues.rs`)

The program's `record_funding`, `record_kamino_rates` and `refresh_nav` take optional
`mock_*` values. Where the keeper gets them is the `feed` setting (`--feed` on the CLI
overrides it; `--mock <path>` implies `mock`):

| `feed` | Program build | Source |
|---|---|---|
| `onchain` | plain | Every mock arg is `None`; the program reads Hawkeye, the Kamino reserve and the oracle accounts named in the config |
| `live` | `mock-venues` | Keeper fetches the public APIs below once per hourly pass and passes `Some(value)` |
| `mock` | `mock-venues` | Keeper reads `mock_path` (see `mock.example.json`), re-read every pass |

Under `live` and `mock`, a missing value means the crank is **skipped** for that vault
that hour (never sent as zero). A production build rejects `Some(..)` with `MockNotAllowed`.

### Live feed endpoints (verified 24 Sep 2026)

| Input | Request | Fields used |
|---|---|---|
| Phoenix funding | `GET https://perp-api.phoenix.trade/v1/funding/{symbol}/rates?limit=3` | latest `rates[].timestamp` (unix s), `fundingRatePercentage` |
| Phoenix market state | `GET https://perp-api.phoenix.trade/v1/view/exchange/markets` | `symbol`, `marketStatus`, `commodityMetadata.status` |
| Phoenix calendar | `GET https://perp-api.phoenix.trade/v1/market/{symbol}/market-calendar` (once per market) | `calendar.weeklySchedule.{Mon..Sun}.sessions[]`, `calendar.dateOverrides.{YYYY-MM-DD}` with `mode == CASH_SESSION` |
| Kamino rates | `GET https://api.kamino.finance/kamino-market/5wJeMrUYECGq41fxRESKALVcHnNX26TAWy4W98yULsua/reserves/metrics` | USDC reserve `borrowApy`, `supplyApy`; per-reserve `totalSupplyUsd / totalSupply` as the price fallback |
| Jupiter price | `GET https://lite-api.jup.ag/price/v3?ids=<mint>,<mint>` | `<mint>.usdPrice` (the v2 endpoint is gone) |

One-line response samples, captured live (full copies under `tests/fixtures/`):

```
funding   {"marketId":4962,"symbol":"TSLA","rates":[{"timestamp":1790186400,"fundingRatePercentage":"0.004209"}, …]}
markets   [{"symbol":"TSLA","assetId":42,"marketStatus":"active","commodityMetadata":{"isCommodity":true,"isAfterHours":false,"status":"active"},"metadata":{"calendar":{"id":"us_equities_extended","nextMarketTransitionUtc":"2026-09-26T00:00:00Z"}}, …}]
calendar  {"market":"TSLA","kind":"equities","calendar":{"weeklySchedule":{"Mon":{"sessions":[{"start":"09:30:00","end":"16:00:00","mode":"CASH_SESSION"}, …]}},"dateOverrides":{"2026-06-19":{"sessions":[{"start":"00:00:00","end":"12:00:00","mode":"INTERNAL"}, …]}}}}
kamino    [{"reserve":"97zoywd8mPZsGTg8q1wdD2Wgkdrs2tqusp1Qqcxbyj7E","liquidityToken":"USDC","liquidityTokenMint":"EPjFWdd5…","borrowApy":"0.0589","supplyApy":"0.0478","totalSupply":"…","totalSupplyUsd":"…"}, …]
jupiter   {"XsDoVfqeBukxuZHWhdvWHBhgEHjGNst4MLodqsJHzoB":{"usdPrice":378.0462244245716,"decimals":8,"stockData":{"id":"xstocks","price":377.94}}}
```

Phoenix lists all nine equities as perps under their bare tickers (`TSLA`, `NVDA`,
`SPY`, `QQQ`, `GOOGL`, `MSTR`, `CRCL`, `HOOD`, `AAPL`), each with a 3600 s funding
interval and the `us_equities_extended` calendar. None is missing.

### Conversions

| Value | Formula |
|---|---|
| funding sample (program unit, hourly bps × 1e6) | `round(fundingRatePercentage × 100 × 1e6)`; e.g. `"0.004209"` → `420_900` (≈ 36.9 % a year: `× 8760 / 1e6` = 3687 bps). Phoenix's sign is positive when longs pay shorts, which is the program's convention |
| `borrow_apy_bps`, `supply_apy_bps` | `round(borrowApy × 10_000)`; `"0.0589"` → `589` |
| `price_e6` | `round(usdPrice × 1e6)`, Jupiter first, Kamino implied price as fallback |
| market open | NYSE cash session (`calendar.rs`) **and** Phoenix `marketStatus == active` **and** the Phoenix calendar puts now inside a `CASH_SESSION` (New York time; a `dateOverrides` entry replaces that day). The stricter wins |

The last fetched values are shown per vault in `/status` under `feed`
(`funding_hourly_scaled`, `funding_ts`, `borrow_bps`, `supply_bps`, `price_e6`,
`phoenix_open`, `fetched_ts`, `source`), `null` under `onchain` and `mock`.

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
  `share_price_stock_e6`, `nav_slot`, `nav_age_slots`, `pending_exit_shares`, `epoch_id`,
  `feed` (live-feed inputs, see "Feeds"; `null` unless `feed = "live"`).
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
| `src/hourly.rs`, `src/fast.rs`, `src/settle.rs` | The three loops |
| `src/alerts.rs`, `src/venues.rs`, `src/chain.rs`, `src/config.rs`, `src/main.rs` | Alerts, input source, RPC, config, CLI |

## Venue blocks in the loops (`venue.rs`, `alt.rs`)

With `program_build = "real"` every engine crank is prepared by `venue::prepare` with only the
blocks its instruction consumes (`NEED_K`, `NEED_P`, `NEED_KP`; `blocks_for_wind_step` /
`blocks_for_unwind_step` per step; table in `docs/CONTRACT.md` "Venue blocks"): the 24-account
Kamino block (both reserves read fresh), the 17-account Phoenix block plus its trader-index accounts
(exchange keys from `GET /v1/view/exchange/keys`, the market's orderbook and spline from
`GET /v1/view/exchange/markets`, cached for an hour), and, for the steps that swap, a Jupiter route
quoted at send time with the vault as authority. Mainnet allows 64 account locks per transaction
(`increase_tx_account_lock_limit` is not active; lookup tables do not raise it) and all three blocks
together are ~60–85 unique accounts, so `Chain::send_ixs` refuses any transaction over 64 before
signing. Swapping steps: `wind_step(1)` USDC→stock for the parked amount (capped by `basis_cap_usdc`),
`unwind_step(3)` stock→USDC for the whole spot leg, `unwind_partial` for the fraction, `size_up`
in Basis for the borrow increment. `VenueData` carries `base_lot_size = 10^(stock_decimals −
baseLotsDecimals)`, `last_valid_slot = now + 150`, `client_order_id = unix time`, and the vault's
cached `phoenix_equity_usdc` (D6; an off-chain Trader decode is the next step). The Kamino and
Phoenix addresses live in a keeper-owned address lookup table per vault (`alt-<SYMBOL>.json`
next to the lease file; created and extended lazily), and `Chain::send_venue` compiles a v0
transaction over that table plus the route's tables.

**Live Phoenix equity (`phoenix_equity.rs`).** On the real build every crank's `VenueData.phoenix_equity_usdc`
is read from the vault's trader account at send time (`venue::live_equity`): the settled
`quote_lot_collateral` (1 quote lot = 1 USDC base unit) less any unsettled funding the vault owes
(`accumulated_funding_for_active_position`, summed over positions; unsettled funding owed *to* the vault
is not counted, so a withdraw never asks for more than Phoenix has settled). The fast loop computes
margin and the emergency check with the same figure, and `/status` reports it per vault as
`phoenix_equity_live_usdc_e6` with `phoenix_funding_pending_usdc_e6` (null before registration or on
the mock build). The vault's cached `phoenix_equity_usdc` is only the program's own running figure.

**Multi-step size-up and partial release.** `size_up_start` (Kamino; Parked stays Parked, Basis →
SizingUp) then `size_up_step(1..3)` with the wind blocks and `size_up_commit`; `unwind_partial_start`
(fraction, reason) then `unwind_partial_step(n, fraction)` with the unwind blocks and
`unwind_partial_commit` (Kamino). The fraction is not stored on chain: the settle loop derives it from
the pending exits on every step, and `resume_partial` does the same for a vault found in
PartialUnwinding (without pending exits it aborts into a full unwind). Both sequences resume from the
vault's `step` in the hourly and settle passes.

One-time setup runs on first sight of a vault each hourly pass (`venue::ensure_setup`): the
Kamino obligation (`init_kamino_obligation`), the vault's token account for the Phoenix
collateral mint, and the Phoenix trader account. Registration alone leaves a trader without the
deposit / withdraw / risk-increase capabilities, so the keeper uses Phoenix's no-referral
onboarding flow, which accepts PDA authorities: `POST /v1/exchange/build-register-ixs`
(`register_trader` + `onboard_trader_delegated`, keeper as fee payer), the keeper signs, and
`POST /v1/exchange/send-register-ixs` adds the Phoenix onboarder's signature and submits
(`venue::onboard_trader`). The keeper pays the trader account's rent. `sync_collateral` is sent whenever the custody token account holds stock. On the
real build `record_kamino_rates` and `refresh_nav` send no keeper values: the program reads the
USDC reserve and refreshes the xStock reserve (`[klend, lending_market, scope_prices]`) itself.

## Venue commands

The program's real Kamino / Jupiter / Phoenix legs take their accounts as remaining-account blocks and
a Borsh `VenueData` (`src/venue_accounts.rs`, mirror of the program's struct; layouts in
`docs/CONTRACT.md` "Venue blocks"). Every engine crank now carries a trailing `venue_data` argument;
the loops send `VenueArgs::none()` (empty) against the mock-venues build, which the deployed program
accepts, and the operator commands below exercise the real blocks:

- `carrera-keeper kamino-block TSLA` — fetches both reserves and prints the vault's 24-account Kamino
  block plus its `venue_data`.
- `carrera-keeper init-obligation TSLA` — sends `init_kamino_obligation` (creates the Kamino user
  metadata, obligation and debt-farm user state; keeper pays rent; idempotent).
- `carrera-keeper jupiter-route TSLA <amount> [--to-stock]` — dry-runs a Jupiter route for the vault
  (quote + swap-instructions with the vault as user, PDA token accounts substituted) and prints the block,
  data size and lookup tables.
- `carrera-keeper venues prove-jupiter --vault TSLA [--amount 1000000] [--dexes Whirlpool] [--out DIR]` —
  the Jupiter proof, see below. Nothing is sent.
- `carrera-keeper venues prove-phoenix --vault TSLA [--out DIR]` — the Phoenix proof, see below. Nothing is sent.
- `carrera-keeper venues setup --vault TSLA` — one-time setup (sends): Kamino obligation, Phoenix collateral
  token account, Phoenix trader registration + onboarding. Idempotent.
- `carrera-keeper venues sync-collateral --vault TSLA` — sends `sync_collateral` when custody holds stock.
- `carrera-keeper venues check --vault TSLA` — prints the setup state (obligation, trader and its
  capabilities, collateral account, custody balance). Used by `deploy/migrate-real-legs.ts --phase post`.

### Jupiter proof (`venues prove-jupiter`, `src/prove.rs`)

The command quotes the route both ways, builds the real-build `wind_step(1)` exactly as the loops would
(Kamino + Phoenix blocks, the Jupiter route as the last block, `VenueData` with `blocks = 7`), runs
`simulateTransaction` (`sigVerify=false`, `replaceRecentBlockhash=true`) against the configured RPC, and
dumps every account the route touches plus the AMM programs it invokes into
`program/tests/fixtures/jupiter/` (`acct_<pubkey>.json`, `program_<pubkey>.json`, `scenario.json`).
The fork test `program/fork-tests/tests/kamino_fork.rs::jupiter_wind_step_one_in_fork` then replays
that route through the plain program in LiteSVM.

Outer metas never mark the vault as a signer (`jupiter_block_from_response`): the program sets
`is_signer` on the CPI's authority index itself and signs with the vault seeds. Marking it in the
transaction would make the keeper's transaction unsignable.

Run of 25 Sep 2026 (mainnet, Helius RPC, TSLA vault `14pBdW5byHDDAXSnhCoortKakZqKYEWwd4i8F4VzR3EM`,
routes restricted to Whirlpool with `--dexes`, see below):

```
USDC→TSLAx: 1000000 in, quoted out 268810, 30 accounts, 38 bytes, tables [E28CeoRY…]
TSLAx→USDC: 26881000 in, quoted out 99920287, 30 accounts, 38 bytes, tables [E28CeoRY…]
wind_step(1): 76 accounts (24 kamino + 19 phoenix + 30 jupiter), venue_data 81 bytes;
  unique locks: 59 with all three blocks, 43 with Kamino + Jupiter (mainnet max 64)
Kamino market lookup table: 8ofreL6hKfEet1DnhHVGvCTnSdz4pg85PpbuCUHnEcKm
simulation [kamino+phoenix+jupiter] failed to fit in a packet without the keeper's per-vault lookup table:
  ... VersionedTransaction too large: 2004 bytes (max: encoded/raw 1644/1232)
simulation [kamino+jupiter (what step 1 consumes)] failed:
  simulation failed: InstructionError(1, Custom(6002))
  Program log: Instruction: WindStep
  Program log: AnchorError ... Error Code: WrongState. Error Number: 6002.
fixtures: 21 accounts, 3 programs, 2 missing → ../program/tests/fixtures/jupiter
```

What this shows: the v0 transaction compiles and is accepted by the RPC (route tables + Kamino's public
market table `8ofreL…` cover the Kamino block; the three-block shape needs the keeper's per-vault table,
which `alt::ensure` creates at cutover, and is not what the loops send anyway), it reaches the deployed
program, and the deployed (mock) program rejects it at its state check before any venue is touched: the
mainnet TSLA vault is in mock Basis, not Winding. The execution proof is the fork:

```
$ cd program/fork-tests && cargo test --release -- --nocapture jupiter
borrowed D = 113.303999 USDC (buffer holds 113.303999 after Kamino's origination fee)
fork clock slot 450424310 vs route dump slot 450418012 (6298 slots apart); route quoted 268810 TSLAx base units per 1 USDC
buffer 113.303999 → 0 USDC, basis_spot_qty 0 → 30445511 TSLAx base units, debt 113.303999 USDC, custody after deposit 0
test jupiter_wind_step_one_in_fork ... ok
```

The plain program patched the route's `in_amount` to the 113.30 USDC borrowed by `wind_start`, computed
`min_out` from the on-chain price and `max_swap_slippage_bps`, executed `shared_accounts_route` through
Orca Whirlpool with the vault PDA as signer, checked the out-mint and `min_out`, recorded
`basis_spot_qty`, and deposited the TSLAx into the Kamino obligation, in 291k CU. The fill
(30 445 511 base units for 113.30 USDC) is within 0.05 % of the 1-USDC quote. Routes are slot-bound and
the AMM state is dumped at one slot, so the fork replays this route only; the fixtures are regenerated
by re-running the command (it clears `acct_*`/`program_*` first). `--dexes Whirlpool` (the default) keeps
the dumped routes replayable: Jupiter's other xStock venues (a quote-time-bound market maker at
`B72M6ny…`) reject a replay at a later slot; `--dexes any` lifts the filter for a plain simulation.

### Phoenix proof (`venues prove-phoenix`, `src/prove.rs`)

The command asks Phoenix to build the vault's trader registration (`POST /v1/exchange/build-register-ixs`
with the vault PDA as `traderAuthority` and the keeper as fee payer), checks the `register_trader`
instruction against the keeper's own builder, simulates collateral-ATA creation + `register_trader` +
`onboard_trader_delegated` on mainnet (partially signed: the onboarder's signature is skipped with
`sigVerify=false`), simulates the real-build `wind_step(3)` shape, and dumps every account the Phoenix
block and the onboarding instructions reference (global configuration, perp asset map, TSLA orderbook and
spline, global vault, canonical mint, trader index and buffer, withdraw queue, Ember state and vault, the
onboarder's permission account) plus the Phoenix and Ember programs into `program/tests/fixtures/phoenix/`.

Run of 25 Sep 2026 (dump slot 450418100, after the Jupiter dump):

```
TSLA: Phoenix market TSLA orderbook 9ZKCwuQD… spline 2QmJ5bTx… base_lots_decimals 3 → base_lot_size 100000 (8 decimals)
trader PDA 5DMGgBh4n9QjPo9NUCT6i2pqrvJcQThGzNvZitjQzJoY (index 0, subaccount 0), collateral ATA BazyyVyU…
trader registered on mainnet: false
Phoenix build-register-ixs: 2 instructions, onboarder EzkM8YbCkBLaCqX2cdxtMyxfTLpKui3mWQWnhe5w2P4Z, include_register_trader true
register_trader from the API matches the keeper's builder (7 accounts)
simulation [create collateral ATA + register_trader + onboard_trader_delegated] succeeded (52 log lines)
wind_step(3): 46 accounts (24 kamino + 19 phoenix), venue_data 47 bytes, 45 unique account locks (mainnet max 64)
simulation [kamino+phoenix wind_step(3)] failed: InstructionError(1, Custom(6002)) WrongState   (mock program, as for Jupiter)
fixtures: 13 accounts, 2 programs, 1 missing → ../program/tests/fixtures/phoenix
```

The fork test `phoenix_basis_cycle_in_fork` then runs the whole basis cycle on the plain program against
the dumped exchange, at the dump slot (Phoenix checks the cluster's `LastRestartSlot` sysvar against the
slot the exchange acknowledged, and its oracle and spline state against the clock, so the test sets the
sysvar from the global configuration and starts its clock 24 h before the dump so the 24 funding samples
land on it):

```
$ cd program/fork-tests && cargo test --release -- --nocapture phoenix
trader capability flags after onboarding: 0x3e
D_b borrowed 34.495304 USDC → Phoenix trader collateral 34495304 quote lots (token account 0 left)
short 30400000 base units = 304 lots against 30444913 spot base units; collateral now 34455680 quote lots
partial release: short 30400000 → 15200000, spot 30444913 → 15222457, D_b 34495304 → 17294096, D 113.303999 → 56.669871 USDC
exit settled in Basis: 50000000 base units to redeem_stock, 0.819542 USDC
after withdraw: D_b 17.294096 → 0.966514 USDC
Parked: debt 56.669871 USDC, parked 55.648354 USDC (round trip cost 1.021517 USDC)
test phoenix_basis_cycle_in_fork ... ok
```

Proven in the fork, in order: Phoenix's own `register_trader` + `onboard_trader_delegated` (capabilities
0x3e: limit, market, risk-increase, risk-reduce, deposit, withdraw); `wind_step(1)` Jupiter buy;
`wind_step(2)` Kamino borrow of D_b = 30 % of the spot notional → Ember wrap → Phoenix `deposit_funds`
(1 quote lot = 1 USDC base unit); `wind_step(3)` IOC short of 304 base lots (0.304 TSLA, all-or-nothing)
filled against the live book; `wind_commit` with the keeper-read equity; then, for an exit of half the
shares requested before the wind, `close_epoch` → `unwind_partial_start(5000)` → `unwind_partial_step(1..3)`
(half the short closed, half the equity withdrawn against half of D_b, half the spot sold with the
proceeds repaying half of D) → `unwind_partial_commit` → `settle_epoch` in Basis paying 0.5 TSLAx from the
obligation and the USDC leg from the Phoenix equity; then guardian `unwind_start`; `unwind_step(1)`
reduce-only IOC close; `unwind_step(2)` `withdraw_funds` of the whole collateral → Ember unwrap → Kamino
repay (0.97 USDC of D_b left: the exit's USDC leg, fees and PnL); `unwind_step(3)` Kamino withdraw +
Jupiter sell; `unwind_commit` folding the residual into the loan and supplying the transit USDC.
Two things the run surfaced and that are now in the code: `withdraw_funds` needs the exchange's
withdraw queue, so the Phoenix block gained it at index 16; and the global configuration and canonical
mint must be writable in the outer transaction. One thing it surfaced about the market: Jupiter's xStock
pools traded ~1.5 % under the Kamino (Scope) oracle at the time, so the sell leg only cleared the
program's oracle floor with `max_swap_slippage_bps = 300` (tier B's 50 would have rejected it; buys pass
at any discount because they deliver more stock). Live, the keeper's `phoenix_equity_usdc` still comes
from the vault's cached field; the fork reads the trader account's `quote_lot_collateral`
(`TraderHeader`, offset 88) and the same read belongs in the keeper before the first live unwind.
