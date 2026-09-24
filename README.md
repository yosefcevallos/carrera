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
