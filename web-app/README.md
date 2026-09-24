# Carrera web app

Landing (`/`), app (`/app`) and ops monitor (`/ops`) for the Carrera xStock overlay vaults. Next.js App Router,
TypeScript, Tailwind v4, zustand + immer with the zeroed-skeleton store pattern.

## Run

```sh
pnpm install
pnpm dev          # http://localhost:3000  (landing at /, app at /app, ops at /ops)
pnpm build && pnpm start
pnpm test         # vitest
pnpm typecheck
```

## Environment

| Variable | Default | Meaning |
|---|---|---|
| `NEXT_PUBLIC_DATA_SOURCE` | `mock` | `mock` = deterministic in-memory world with a demo wallet; `rpc` = read program accounts from Solana |
| `NEXT_PUBLIC_RPC_URL` | `http://127.0.0.1:8899` | RPC endpoint (rpc mode) |
| `NEXT_PUBLIC_PROGRAM_ID` | placeholder | `carrera_overlay` program id; set after `anchor keys sync` in `../program` |
| `NEXT_PUBLIC_USDC_MINT` | mainnet USDC | USDC mint |
| `NEXT_PUBLIC_XSTOCK_MINTS` | `{}` | JSON `{"TSLA":"<mint>",...}`; unset tickers use placeholder PDAs |
| `NEXT_PUBLIC_APP_HOST` | `app.carrera.xyz` | Host that is rewritten to `/app` (see below) |
| `NEXT_PUBLIC_KEEPER_URL` | `http://127.0.0.1:8787` | Keeper status server (`status_bind` in `keeper/README.md`); used by `/ops` in rpc mode |
| `NEXT_PUBLIC_OPS_ALLOWED_WALLETS` | empty | Comma-separated pubkeys allowed to open `/ops` in rpc mode; mock mode is open |

## Host routing

`src/proxy.ts` rewrites requests whose `Host` starts with `NEXT_PUBLIC_APP_HOST` to `/app/...`.
Any other host serves the landing at `/`. On localhost both are reachable by path.

## Data sources

- **mock** (`src/lib/mock/`): seeded world derived from `docs/frontend-handoff/vaults.sample.json`
  shapes with 90 days of `sharePriceHistory` and mode bands per vault, a demo wallet
  (`7xKX…gAsU`) and sped-up accrual/settlement (8 s epochs). The wallet chip connects the demo
  wallet directly; no extension is needed.
- **rpc** (`src/lib/chain/`): decodes `Registry` and the nine `OverlayVault` accounts with Borsh per
  `docs/CONTRACT.md` (`layout.ts`), derives PDAs (`pda.ts`), builds `deposit` / `request_exit` /
  `cancel_exit` / `redeem` instructions with Anchor discriminators (`ix.ts`), and signs through
  wallet-adapter (`actions.ts`, `src/lib/use-signer.ts`). Phantom and Solflare adapters are
  registered; Backpack arrives through Wallet Standard. Share-price history, 24h payouts and
  depositor counts need the indexer and are zero in rpc mode until it exists; pending exits
  likewise (ExitRequest PDAs are nonce-keyed).

## Ops monitor (`/ops`)

Keeper and strategy monitor: every vault is a book with its legs (long spot pledged on
Kamino, USDC borrowed on Kamino, USDC supplied on Kamino when parked, short perp on
Phoenix), net delta, estimated carry PnL, basis, liquidation distances, the allocation rule
inputs with enter/exit bands, LTV and margin gauges, a 7-day funding-vs-hurdle chart with
the vault's state underneath, keeper health and alerts. Manual cranks (rebalance to Kamino,
rebalance to Phoenix, emergency unwind, pause) build the instructions from `docs/CONTRACT.md`
and send them through the connected keeper or guardian wallet in rpc mode.

Data comes from the keeper's HTTP status server, not from RPC: `GET /status` every 10 s,
`GET /history?vault=<T>&hours=168` for the selected book every 60 s, `GET /healthz` with each
status poll. Wire types mirror `keeper/src/status.rs` (`src/lib/ops-types.ts`). The store
(`src/store/ops-store.ts`) is a zeroed skeleton keyed by the nine tickers plus a keeper block;
`OpsSyncer` is mounted on the `/ops` route only, since the keeper is not reachable from the
public pages. Liquidation prices use the keeper's formulas (Kamino `price × ltv / liq_ltv`,
Phoenix `mark × (1 + tier buffer)`); the keeper does not send token decimals, so the page
assumes 8 for xStocks and 6 for USDC (`src/lib/ops-math.ts`).

- **mock** (`src/lib/mock/ops.ts`): deterministic books covering every state the page renders:
  four in Basis (TSLA with margin near its floor), MSTR parked, three Idle, NVDA stuck in
  Winding at step 2 for 22 minutes so the keeper's "stuck" alert shows. Seven days of history
  at five-minute resolution per book. Cranks mutate the mock and record an alert.
- **rpc**: fetches the keeper. Books the keeper has not reported stay zeroed.

## State

Follows the zeroed-skeleton pattern: `src/store/<domain>-store.ts` (vanilla `createStore` +
immer, State/Actions interfaces, skeleton from `src/constants/vaults.ts`, `reset*`),
`src/store/<domain>-provider.tsx` (one store per React tree, selector hook), headless syncers in
`src/components/{VaultSyncer,PositionSyncer}.tsx`, fetchers in `src/lib/fetch-*.ts`, and
`src/lib/use-refresh.ts` to re-run the same fetchers and setters after an action.

Stores: `vault` (protocol stats + nine vault records), `wallet` (status, address, demo flag),
`position` (balances, positions, pending exits per ticker), `ui` (selection, filter, open vault,
tab, chart range, toast), `ops` (keeper block, nine books, per-book history, selected book, health). Derived values (trailing yield, hurdle bands, redemption preview) are
computed in `src/lib/yield.ts`, never stored.

## Copy and behaviour changes vs the v0.3 frontend handoff

Per `docs/handover/03-FRONTEND-RECONCILIATION.md` and `docs/DECISIONS.md`:
mode labels are Funding / Parked (USDC supplied on Kamino) / Idle (loan repaid); every projected
"you'd earn" figure is gone, replaced by trailing realised share-price growth (7d / 30d / since
inception, since-inception only for vaults under 7 days); the vault window shows the live hurdle
and enter/exit bands as facts; deposits are stock-only; the withdraw preview handles the early-life
case where USDC is zero and the stock leg is slightly reduced.
