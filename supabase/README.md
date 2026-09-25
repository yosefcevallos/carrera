# Carrera database (Supabase)

Imperative migrations in `migrations/`, seed in `seed.sql`. Written by `../indexer`, read by `../web-app`.

```bash
npx supabase@2 start          # local stack (Docker)
npx supabase@2 db reset       # apply migrations + seed
npx supabase@2 db advisors    # security / performance findings
npx supabase@2 db lint
npx supabase@2 stop
```

Hosted project: `npx supabase@2 link --project-ref <ref>` then `npx supabase@2 db push`.

## Tables

| Table | Written from | Public read |
|---|---|---|
| `vaults` | seed / deploy script | yes |
| `program_events` | every decoded event, raw | no |
| `nav_samples`, `rule_samples`, `funding_samples` | NavRefreshed, RuleEvaluated, FundingRecorded | yes |
| `state_changes`, `rebalances`, `deposits`, `fees` | matching events | yes |
| `epochs`, `exits` | EpochClosed/Settled, ExitRequested/Cancelled, Redeemed | yes |
| `keeper_snapshots`, `keeper_alerts`, `keeper_heartbeats` | keeper `/status` poller | no |

Views (`security_invoker`): `v_trailing_yield` (7d / 30d / inception growth of share price per
vault), `v_funding_24h` (last 24 hourly samples per vault), `v_protocol_stats` (TVL, TVL-weighted
funding and parked APY, vault counts by state, USDC paid out in 24h, distinct depositors).

## Security model

RLS is enabled on every table. `anon` and `authenticated` get `select` only, and only on the
public tables and views. `program_events` and `keeper_*` have no policies and no grants for those
roles. Writes happen through `service_role`, which bypasses RLS and is never sent to a browser.
Grants are explicit because new Supabase projects no longer expose `public` tables automatically.

## Funding history sources

`funding_samples` receives rows from two writers: the on-chain `FundingRecorded` event (stamped
with block time; tonight's 24-sample backfill landed within a few minutes of each other) and the
indexer's Phoenix poller (`indexer/src/sources/phoenix.ts`, hour-stamped, 168 rows per vault every
hour). `v_funding_24h` and `v_funding_7d` return the latest rows by `ts`, so the Phoenix series
dominates once it exists. Rows are keyed on `(vault_symbol, ts)`; the two sources never collide.
