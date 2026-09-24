# Frontend reconciliation — v0.3 frontend spec vs v0.4.1 product spec

v0.4.1 is authoritative. Resolutions:

| Topic | Old frontend spec | Resolution |
|---|---|---|
| Parked mode copy | "lends its USDC on Kamino" | Rewrite all copy around ONyc: mode badge, notes, legend, calculator fine print, detail panel. Kamino USDC supply is not used anywhere. |
| Threshold copy | fixed 4.5% placeholder | Replace with the live hurdle (ONyc trailing yield + L·r + amortised cost) and the enter/exit bands. Show the hurdle as a number read from chain (`RuleEvaluated` events / vault account), not a constant. |
| Projected yield | "You'd earn …" in calculator and deposit form | Remove. Show trailing realised growth of share price (7d / 30d / since inception). Vaults < 7 days old show "since inception" only. Mode and hurdle may be shown as facts, never as an APY promise. |
| App structure | single page + modal | Launch scope: `/` (vault grid), `/vault/[symbol]`, `/portfolio` (required: users must see exit status and claim), `/activity`. `/docs` and `/admin` may ship after canary. Single-page-plus-modal is acceptable as the interim canary UI. |
| Data stack | React Query | Same library as TanStack Query in v0.4.1; no conflict. WebSocket subscriptions on vault accounts are required for mode/NAV; the Supabase/Helius indexer is required for history and activity but may be polling-based at canary. `@solana/kit` + Codama client is the target; a wallet-adapter bridge is fine at canary. |
| USDC deposits | none | Agreed for launch: stock-only deposits. `deposit_usdc` is deferred to v1.1 and marked so in the spec. |
| Redemption formula | USDC part never decreases | v0.4.1 wins: USDC out can be zero with the stock leg slightly reduced during a vault's early life. UI must render this case with a one-line explanation. |

## Change in v0.4.1 vs v0.4
Allocation-rule cost term corrected. It is now `cost_apy = roundtrip_cost_bps × 8760 / expected_hold_hours`, added to `y_onyc + L·r` (all annualised on basis notional). The extra division by L was a typo. `expected_hold_hours` default raised from 168 to 720; with 60 bps round trip that gives a hurdle ≈19% and an entry level ≈21% annualised funding at today's ONyc and Kamino rates.
