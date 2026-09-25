# Carrera

xStock funding overlay vaults on Solana. One vault per tokenised equity; depositors keep
the stock's price exposure and earn USDC from a borrowed slice that runs a Phoenix
basis trade when funding is high and sits on Kamino otherwise.

| Folder | What | Toolchain |
|---|---|---|
| `program/` | Anchor program `carrera_overlay`: vaults, shares, NAV, exit epochs, allocation rule, state machine | Anchor 0.32, Rust |
| `keeper/` | Off-chain crank: hourly rule loop, 60 s rebalance loop, leader lease, alerts | Rust |
| `web-app/` | Landing and app UI | Next.js, zustand + immer |
| `docs/` | Product and technical specs, decisions, program interface contract | – |

Start with `docs/handover/00-README.md`, then `docs/DECISIONS.md`, then `docs/CONTRACT.md`.
Each folder has its own README with build and test commands.

## Mainnet (deployed 24 Sep 2026)

| | Address |
|---|---|
| Program `carrera_overlay` (mock-venues build) | `GH45ANLzg1t6rNnaoNqE39rN1rKGXFQXPZnNxvbhUmYw` |
| Registry | `GD5kv4szv28VTnhWRAZCA875uSbLLMc1uESyZj49SB1b` |
| Admin / upgrade authority / guardian | `DMi239MAp1ZEV7mrh56u7Muz5tV1MHeULxSoyvGU8Y6H` |
| Keeper hot key | `GAJubsxguMtKmZKTxUqgRY1FjLcFdgpsDNsP3eQFKgWk` |
| Kamino xStocks market | `5wJeMrUYECGq41fxRESKALVcHnNX26TAWy4W98yULsua` |

Vault PDAs and share mints: `deploy/pdas.ts` (also in the Supabase `vaults` table). Per-vault LTVs follow `docs/DECISIONS.md` D7 (40% drop buffer); deposit caps $25k per
vault. The venue legs (Jupiter, Kamino, Phoenix) are mock adapters in this build: the keeper feeds real
Phoenix funding, Kamino rates and Jupiter prices on-chain, the state machine runs on them, but no real
borrow, swap or perp position is opened. See `docs/DECISIONS.md` D4.
