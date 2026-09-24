# Carrera Markets — xStock Funding Overlay Vaults: engineering handover

Date: 24 Sep 2026 · Spec version: v0.4.1

## Contents
- `00-README.md` — this file
- `01-one-pager-and-spec-v0.4.md` — Part A product one-pager, Part B technical spec (program, keeper, web app, params, milestones)
- `02-CHANGELOG.md` — how the design evolved and why (useful context for decisions that look odd in isolation)
- `03-FRONTEND-RECONCILIATION.md` — resolutions for conflicts with the older v0.3 frontend spec

## Reading order
1. Part A (one-pager) for the product shape.
2. Part B §2 (allocation rule), §4 (accounts, NAV), §5 (instruction table with pre/post states), §6 (state machine, tiers, rebalancing).
3. §7 (oracles and trade bounds) before writing any CPI code.
4. §10 verification items — resolve Q1 (ONyc acquisition path) and Q2 (Phoenix maintenance margins) before M1.

## External references used
- Phoenix perps docs: https://docs.phoenix.trade (funding, market calendar, leverage tiers)
- Phoenix on-chain CPI SDK (`phoenix-rise`, Hawkeye views, LiteSVM fixtures): https://docs.phoenix.trade/sdk/on-chain-programs
  and https://github.com/Ellipsis-Labs/rise-public
- Kamino Lend xStocks market (collateral LTVs, USDC borrow/supply rates)
- OnRe ONyc on Kamino (reinsurance-backed yield token; Chainlink NAV feed)

## Decisions already made (do not reopen without product sign-off)
- Deposit asset is the xStock; depositor keeps full price exposure; one vault per equity.
- The loan is always outstanding (except under guard/pause/emergency) and lives in exactly one of two modes: Basis (spot long + Phoenix short) or Parked (ONyc). Kamino's plain USDC supply market is not used.
- Mode switch rule is evaluated on-chain from data the program reads itself; the keeper is a crank, not a decision-maker.
- Exits go through hourly epochs; redemption pays stock + USDC pro rata at the epoch's NAV.
