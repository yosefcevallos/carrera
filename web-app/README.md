# Carrera web app

Landing (`/`) and app (`/app`) for the Carrera xStock overlay vaults. Next.js App Router,
TypeScript, Tailwind v4, zustand + immer with the zeroed-skeleton store pattern.

## Run

```sh
pnpm install
pnpm dev          # http://localhost:3000  (landing at /, app at /app)
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

## State

Follows the zeroed-skeleton pattern: `src/store/<domain>-store.ts` (vanilla `createStore` +
immer, State/Actions interfaces, skeleton from `src/constants/vaults.ts`, `reset*`),
`src/store/<domain>-provider.tsx` (one store per React tree, selector hook), headless syncers in
`src/components/{VaultSyncer,PositionSyncer}.tsx`, fetchers in `src/lib/fetch-*.ts`, and
`src/lib/use-refresh.ts` to re-run the same fetchers and setters after an action.

Stores: `vault` (protocol stats + nine vault records), `wallet` (status, address, demo flag),
`position` (balances, positions, pending exits per ticker), `ui` (selection, filter, open vault,
tab, chart range, toast). Derived values (trailing yield, hurdle bands, redemption preview) are
computed in `src/lib/yield.ts`, never stored.

## Copy and behaviour changes vs the v0.3 frontend handoff

Per `docs/handover/03-FRONTEND-RECONCILIATION.md` and `docs/DECISIONS.md`:
mode labels are Funding / Parked (USDC supplied on Kamino) / Idle (loan repaid); every projected
"you'd earn" figure is gone, replaced by trailing realised share-price growth (7d / 30d / since
inception, since-inception only for vaults under 7 days); the vault window shows the live hurdle
and enter/exit bands as facts; deposits are stock-only; the withdraw preview handles the early-life
case where USDC is zero and the stock leg is slightly reduced.
