# Carrera

Yield on tokenised stocks. One vault per xStock on Solana: deposit the TSLAx you already hold,
keep every move in its price, and earn USDC from a borrowed slice that runs a delta-neutral
funding trade on Phoenix when funding is high and sits on Kamino otherwise.

Live on mainnet since 24 Sep 2026. Program `GH45ANLzg1t6rNnaoNqE39rN1rKGXFQXPZnNxvbhUmYw`.

## What is real and what is simulated in this build

| Real, on mainnet today | Simulated in this build |
|---|---|
| Anchor program with nine vaults, Token-2022 deposits and redemptions, share accounting, NAV, hourly exit epochs | The Kamino borrow, the Jupiter swap and the Phoenix short are accounting adapters: the program records the legs it would hold, no external position is opened |
| On-chain allocation rule: Basis when the 24h Phoenix funding average clears a hurdle built from Kamino rates and round-trip cost, with hysteresis | The keeper is the price and funding oracle: it fetches Phoenix, Kamino and Jupiter and writes the values on chain (registered keepers only) |
| Keeper: hourly rule loop, 60 s NAV and health loop, 5 min exit settlement, leader lease | No USDC is actually earned; the "earned" figures are what the recorded legs would pay at live rates |
| Indexer: every program event plus seven days of Phoenix funding into Supabase | |
| Web app: landing, vault table, deposit / withdraw / requests ticket, ops monitor | |

The real venue legs are on the [`real-legs`](https://github.com/yosefcevallos/carrera/tree/real-legs)
branch. The Kamino leg (obligation, collateral, borrow, supply, repay, withdraw) is verified against
forked mainnet state with the live TSLAx and USDC reserves. Jupiter and Phoenix are wired and
encoding-tested. See `program/README.md` there for what is still open.

## Architecture

- `program/` Anchor program `carrera_overlay`: registry, vault PDAs, share mints, allocation rule, state machine (Idle → Parked → Winding → Basis → Unwinding), exit epochs. Venue calls go through adapters in `src/venues/`.
- `keeper/` Rust crank with a live feed (Phoenix funding, Kamino rates, Jupiter prices), an HTTP status server for the ops page, and a file-based leader lease.
- `indexer/` TypeScript service: decodes program events into Postgres, polls the keeper for snapshots, pulls Phoenix funding history.
- `supabase/` schema, RLS (public read, keeper tables private), trailing-yield and funding views.
- `web-app/` Next.js with zustand + immer zeroed-skeleton stores; RPC reads through a server-side proxy so no key reaches the browser.
- `deploy/` init, tiers, wind and settle scripts; `ops/` launchd services.

## Sixty-second demo

1. Open the landing page: the pole position block and the live grid rank the nine vaults by current APY from chain.
2. Launch app, connect a wallet holding an xStock.
3. Open a vault, deposit. The row updates the moment the transaction confirms.
4. Withdraw part of it: the request appears under Requests with its settlement tracker. The keeper settles the epoch within five minutes of the hour.
5. Claim to wallet: the stock returns; the USDC leg reads zero in this build.
6. Open `/ops`: every vault as a book with its three legs, net delta, carry and the rule inputs.

## Decisions that shape the design

`docs/DECISIONS.md` records them: Parked mode is Kamino supply (D1), the allocation rule (D2), the
4.5% question (D3), the venue adapter boundary (D4), frontend reconciliation (D5), keeper as Phoenix
oracle instead of Hawkeye (D6), and per-vault LTV from a uniform 40% drop buffer using the real
Kamino and Phoenix parameters (D7). The program interface is in `docs/CONTRACT.md`; the original
product and technical spec is in `docs/handover/`.

## Mainnet

| | Address |
|---|---|
| Program `carrera_overlay` (mock-venues build) | `GH45ANLzg1t6rNnaoNqE39rN1rKGXFQXPZnNxvbhUmYw` |
| Registry | `GD5kv4szv28VTnhWRAZCA875uSbLLMc1uESyZj49SB1b` |
| Admin, upgrade authority, guardian | `DMi239MAp1ZEV7mrh56u7Muz5tV1MHeULxSoyvGU8Y6H` |
| Keeper hot key | `GAJubsxguMtKmZKTxUqgRY1FjLcFdgpsDNsP3eQFKgWk` |
| Kamino xStocks market | `5wJeMrUYECGq41fxRESKALVcHnNX26TAWy4W98yULsua` |

Vault PDAs and share mints: `deploy/pdas.ts`, also in the Supabase `vaults` table. Deposit caps $25k per vault.

## Running it

| Component | Command |
|---|---|
| Program | `cd program && PATH=~/.local/share/solana/install/releases/3.1.1/solana-release/bin:$PATH anchor build -- --features mock-venues && anchor test --skip-build` |
| Keeper | `cd keeper && cargo run --release -- --config ~/.config/carrera/keeper.toml run --feed live` |
| Indexer | `cd indexer && pnpm build && node --env-file=.env dist/main.js` |
| Web app | `cd web-app && pnpm dev` with `.env.local` from the README there |
| Services | `ops/install-services.sh` installs keeper and indexer as launchd agents |

| Folder | Toolchain |
|---|---|
| `program/` | Anchor 0.32, Solana CLI 3.1.1, Rust |
| `keeper/` | Rust |
| `indexer/`, `web-app/` | Node 24, pnpm |

Carrera is a hackathon prototype. Rates and yields shown are current market rates applied to the
vault's accounting; venue execution is simulated in this build. Not financial advice.
