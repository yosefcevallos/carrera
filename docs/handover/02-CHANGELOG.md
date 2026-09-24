# Changelog

## v0.4.1
- Fixed the allocation-rule cost term (typo divided by L; units now consistent). `expected_hold_hours` default 720. USDC deposits deferred to v1.1. Frontend reconciliation notes added.

## v0.4 (current)
- Parked mode now buys and holds OnRe's ONyc instead of supplying USDC on Kamino (Kamino USDC supply ≈4.8% is below the ≈5.9% borrow rate, so that carry was negative).
- Hurdle for entering Basis is ONyc's trailing yield (derived on-chain from a daily NAV ring buffer) plus the secondary loan's interest plus amortised round-trip cost.
- Added `record_onyc_nav`, `rebalance_from_parked`, ONyc trade bounds vs NAV, negative-carry guard (`repay` to Idle when ONyc yield < borrow rate + margin).
- Added instruction pre/post-state table and parameter defaults table for implementation.

## v0.3
- "Off" state changed from debt-free to Parked (loan kept and supplied on Kamino USDC) so Basis can resume without re-borrowing; rule compared funding against Kamino USDC yield.

## v0.2
- Product changed from a USDC-denominated basis vault to per-equity overlay vaults: users deposit the xStock, keep upside, and earn on a borrowed slice. Redemption pays stock + USDC.

## v0.1
- USDC vault running a delta-neutral basis trade across the nine equities listed on both Kamino xStocks and Phoenix; parked in Kamino USDC when funding was low. Superseded.
