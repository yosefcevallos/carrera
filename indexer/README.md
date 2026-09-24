# Carrera indexer

Decodes `carrera_overlay` events into Postgres (Supabase) and records keeper ops snapshots.
Schema and RLS live in `../supabase/`. Event layouts are in `EVENTS.md`.

## Run

```bash
cp .env.example .env   # fill in SUPABASE_URL and SUPABASE_SERVICE_ROLE_KEY
pnpm install
pnpm dev               # tsx watch
pnpm build && pnpm start
pnpm test
```

Vault pubkey → symbol mapping is loaded from the `vaults` table at startup, so seed it with
the deployed program's PDAs first (`supabase/seed.sql` has placeholders).

## Sources

| `SOURCE` | How | When |
|---|---|---|
| `poll` | `getSignaturesForAddress(PROGRAM_ID)` every `POLL_INTERVAL_MS`, oldest-first from the cursor in `CURSOR_PATH` | localnet, devnet, or any RPC without Helius |
| `helius` | HTTP `POST /webhook/helius` on `PORT`; raw payloads are decoded directly, enhanced payloads are refetched by signature over `RPC_URL` | mainnet / devnet with a Helius account |

Both paths are idempotent: `program_events` is unique on `(signature, log_index)` and every
derived table upserts on its natural key, so replays and retries are safe.

If `KEEPER_URL` is set, `GET {KEEPER_URL}/status` is polled every `KEEPER_INTERVAL_MS` and
written to `keeper_snapshots`, `keeper_heartbeats` and `keeper_alerts` (an alert is inserted
once and marked resolved when the keeper stops reporting it).

## Helius webhook setup

1. Deploy this service somewhere reachable over HTTPS with `SOURCE=helius`.
2. In the Helius dashboard (or `POST https://api.helius.xyz/v0/webhooks?api-key=…`), create a webhook:
   `webhookType: "raw"` (preferred, includes logs), `transactionTypes: ["ANY"]`,
   `accountAddresses: ["<PROGRAM_ID>"]`, `webhookURL: "https://<host>/webhook/helius"`,
   `authHeader: "<HELIUS_WEBHOOK_SECRET>"`.
3. Set the same `HELIUS_WEBHOOK_SECRET` here. Requests whose `Authorization` header differs get 401.
4. Run one `SOURCE=poll` pass first to backfill history before the webhook was registered.

## Access model

- The indexer and keeper poller write with the **service role key** (bypasses RLS). Server-side only.
- The web app reads with the **anon / publishable key**. RLS grants `select` on the public tables
  (`vaults`, `nav_samples`, `rule_samples`, `funding_samples`, `state_changes`, `rebalances`,
  `epochs`, `exits`, `deposits`, `fees`) and the views `v_trailing_yield`, `v_funding_24h`,
  `v_protocol_stats`.
- `program_events` and the `keeper_*` tables have RLS enabled with no policies and no grants for
  `anon`/`authenticated`, so they are unreachable from the browser. The ops page must read them
  through a server route using the service role, or you add an `authenticated` policy scoped to
  ops users later.
