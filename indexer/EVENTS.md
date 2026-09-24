# Event layouts

Source of truth: `program/programs/carrera_overlay/src/events.rs` and
`program/target/idl/carrera_overlay.json`. Discriminator = `sha256("event:<Name>")[0..8]`,
then Borsh fields in this order. `pubkey` = 32 bytes, integers little-endian.

| Event | Discriminator | Fields |
|---|---|---|
| `Deposited` | `6f8d1a2da1236439` | vault pubkey, user pubkey, qty u64, shares u64 |
| `ExitRequested` | `5c7d0617e9c49546` | vault, user, nonce u64, shares u64, epoch_id u64 |
| `ExitCancelled` | `58d40495ac4089e8` | vault, user, nonce u64, shares u64 |
| `EpochClosed` | `150430381da94d2a` | vault, epoch_id u64, shares_total u64, stock_owed u64, usdc_owed u64 |
| `EpochSettled` | `20db2d9cfa73beff` | vault, epoch_id u64, stock_paid u64, usdc_paid u64 |
| `Redeemed` | `0e1db7471fa56b26` | vault, user, nonce u64, shares u64, stock u64, usdc u64 |
| `StateChanged` | `009637e02b1ad6c0` | vault, from u8, to u8, step u8 |
| `RuleEvaluated` | `ad31fed90b140ab3` | vault, f_avg_bps i64, parked_apy_bps u32, r_bps u32, hurdle_bps i64, decision u8 |
| `NavRefreshed` | `82ea401109eea128` | vault, nav_usd_e6 u64, share_price_stock_e6 u64, price_e6 u64 |
| `FundingRecorded` | `7b54d1f3b3a8c24d` | vault, rate_bps_e6_hourly i64, f_avg_bps i64, samples u8 |
| `KaminoRatesRecorded` | `e09f480d5a7fbef7` | borrow_apy_bps u32, supply_apy_bps u32 |
| `Rebalanced` | `4a6539f4b5b334b6` | vault, kind u8 (0 to_kamino, 1 to_phoenix, 2 from_parked), amount u64 |
| `FeeCrystallised` | `91335a877cf49d5a` | vault, shares u64, high_water_e6 u64 |
| `Paused` | `acf805fd31ffffe8` | (none) |
| `Unpaused` | `9c962fae78d85d75` | (none) |

## Mapping to tables

| Event | Table | Note |
|---|---|---|
| `NavRefreshed` | `nav_samples` | |
| `FundingRecorded` | `funding_samples` | |
| `RuleEvaluated` | `rule_samples` | `state` = the `to` of the last `StateChanged` in the same tx, else the latest recorded state |
| `StateChanged` | `state_changes` | |
| `Rebalanced` | `rebalances` | |
| `Deposited` | `deposits` | |
| `FeeCrystallised` | `fees` | |
| `EpochClosed` / `EpochSettled` | `epochs` | per-share values derived as `paid × 1e6 / shares_total` |
| `ExitRequested` | `exits` | keyed on (vault, user, nonce); request signature kept |
| `ExitCancelled` / `Redeemed` | `exits` | matched on (vault, user, nonce) |
| everything | `program_events` | raw, JSON payload, unique on (signature, log_index) |
