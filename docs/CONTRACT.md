# Program interface contract

The keeper and web app build against this file, not against the program source.
Program crate: `carrera_overlay` (Anchor 0.32). Discriminators are Anchor's
default: `sha256("global:<instruction_name>")[0..8]` for instructions and
`sha256("account:<StructName>")[0..8]` for accounts. Args are Borsh in the order
listed. Numbers: token amounts `u64` in base units; rates `u32`/`i64` in bps;
prices `u64` with 6 decimals (`_e6`).

## PDAs (program-derived, `find_program_address`)

| Account | Seeds |
|---|---|
| `Registry` | `["registry"]` |
| `OverlayVault` | `["vault", xstock_mint]` |
| share mint | `["shares", vault]` |
| stock custody token account | `["stock", vault]` |
| USDC buffer token account | `["usdc", vault]` |
| redemption stock token account | `["redeem_stock", vault]` |
| redemption USDC token account | `["redeem_usdc", vault]` |
| `ExitRequest` | `["exit", vault, user, nonce: u64 LE]` |
| `ExitEpoch` | `["epoch", vault, epoch_id: u64 LE]` |

## Enums

```
VaultState: Idle=0, Parked=1, Winding=2, Basis=3, Unwinding=4, SizingUp=5, PartialUnwinding=6
UnwindReason: Rule=0, ExitDemand=1, Emergency=2
Tier: A=0, B=1, C=2, D=3
ExitStatus: Open=0, Settled=1, Redeemed=2, Cancelled=3
```

## Accounts (Anchor `#[account]`, field order is the Borsh order)

```
Registry {
  admin: Pubkey, guardian: Pubkey, keepers: [Pubkey; 4], keeper_count: u8,
  paused: bool, usdc_mint: Pubkey,
  // Kamino USDC reserve rates, annualised bps, written by record_kamino_rates
  borrow_apy_bps: u32, supply_apy_bps: u32, rates_slot: u64,
  bump: u8,
}

VaultParams {
  ltv_bps: u32, min_margin_bps: u32, liq_ltv_bps: u32, emergency_ltv_bps: u32,
  enter_margin_bps: u32, exit_margin_bps: u32, carry_guard_margin_bps: u32,
  min_enter_funding_bps: u32, expected_hold_hours: u32, roundtrip_cost_bps: u32,
  funding_window: u8,                       // W, default 24
  max_swap_slippage_bps: u32, max_perp_slippage_bps: u32, max_index_dev_bps: u32,
  hedge_tol_bps: u32, size_band_bps: u32,
  rebalance_ltv_band_bps: u32, rebalance_margin_band_bps: u32,
  max_nav_age_slots: u64, epoch_len_secs: u64,
  perf_fee_bps: u32, exit_fee_bps: u32,
  deposit_cap_stock: u64, basis_cap_usdc: u64,
}

OverlayVault {
  xstock_mint: Pubkey, share_mint: Pubkey, tier: u8, params: VaultParams,
  state: u8, step: u8, market_open: bool,
  collateral_qty: u64, basis_spot_qty: u64, debt_usdc: u64, debt_b_usdc: u64,
  parked_usdc: u64,                         // supplied on Kamino
  phoenix_equity_usdc: u64, phoenix_short_qty: u64,
  funding: [i64; 24], funding_head: u8, funding_samples: u8, last_funding_ts: i64,
  nav_usd_e6: u64, share_price_stock_e6: u64, price_e6: u64, nav_slot: u64,
  high_water_e6: u64,
  total_shares: u64, pending_exit_shares: u64, epoch_id: u64, epoch_opened_ts: i64,
  last_rule: RuleEvaluation, stock_decimals: u8, bump: u8,
}

RuleEvaluation { f_avg_bps: i64, parked_apy_bps: u32, r_bps: u32, hurdle_bps: i64, decision: u8, ts: i64 }   // hurdle_bps = break-even r + L·r since D8; the 3h average is only in the event
// decision: 0 none, 1 to_basis, 2 to_parked, 3 to_idle

ExitRequest { vault: Pubkey, user: Pubkey, nonce: u64, shares: u64, epoch_id: u64, status: u8, bump: u8 }
ExitEpoch  { vault: Pubkey, id: u64, shares_total: u64, stock_owed: u64, usdc_owed: u64,
             stock_per_share_e6: u64, usdc_per_share_e6: u64, closed: bool, settled: bool, bump: u8 }
```

## Instructions

Accounts are listed in order. `registry` and `vault` are always the PDAs above.

| Name | Args | Signer | Other accounts |
|---|---|---|---|
| `init_registry` | `guardian: Pubkey, keepers: Vec<Pubkey>` | admin (payer) | registry, usdc_mint, system_program |
| `set_roles` | `admin: Option<Pubkey>, guardian: Option<Pubkey>, keepers: Option<Vec<Pubkey>>` | admin | registry |
| `pause` / `unpause` | – | admin or guardian (pause), admin (unpause) | registry |
| `init_vault` | `tier: u8, params: VaultParams` | admin (payer) | registry, vault, xstock_mint, share_mint, stock_custody, usdc_buffer, redeem_stock, redeem_usdc, usdc_mint, token_program, system_program, rent, stock_token_program |
| `set_params` | `params: VaultParams` | admin | registry, vault |
| `set_market_open` | `open: bool` | keeper | registry, vault |
| `deposit` | `qty: u64, min_shares: u64` | user | registry, vault, share_mint, xstock_mint, stock_custody, user_stock, user_shares, token_program, stock_token_program |
| `request_exit` | `shares: u64, nonce: u64` | user (payer) | registry, vault, exit_request, exit_epoch (current open, init_if_needed), share_mint, user_shares, escrow_shares(`["escrow", vault]`), token_program, system_program |
| `cancel_exit` | – | user | vault, exit_request, exit_epoch, share_mint, user_shares, escrow_shares, token_program |
| `redeem` | – | user | vault, exit_request, exit_epoch, share_mint, escrow_shares, redeem_stock, redeem_usdc, user_stock, user_usdc, token_program, xstock_mint, stock_token_program |
| `record_funding` | `mock_rate_bps_hourly: Option<i64>` (required; keeper-supplied on every build, DECISIONS D6) | keeper | registry, vault, hawkeye_view (unchecked, unused) |
| `record_kamino_rates` | `mock_borrow_bps: Option<u32>, mock_supply_bps: Option<u32>` | anyone (keeper when values supplied) | registry, kamino_reserve (the USDC reserve; non-mock builds derive borrow/supply APR from it) |
| `refresh_nav` | `mock_price_e6: Option<u64>` | anyone (keeper when a value is supplied) | registry, vault, oracle (non-mock: the vault's xStock reserve); remaining `[klend_program, lending_market, scope_prices]` triggers `refresh_reserve` first |
| `park` | `venue_data: Vec<u8>` | keeper | registry, vault + venue blocks |
| `repay` | `venue_data: Vec<u8>` | keeper or guardian | registry, vault + venue blocks. Valid from Parked or Idle; draws `usdc_buffer` first, then supplied USDC; dust written off |
| `sync_collateral` | `venue_data: Vec<u8>` | keeper | registry, vault + Kamino block. Deposits all custody stock into the obligation (no-op on mock builds) |
| `init_kamino_obligation` | `venue_data: Vec<u8>` | keeper (payer) | registry, vault (writable), system_program, rent + Kamino block, user_metadata PDA. Idempotent |
| `wind_start` | `venue_data: Vec<u8>` | keeper | registry, vault + venue blocks |
| `wind_step` | `n: u8, venue_data: Vec<u8>` | keeper | registry, vault + venue blocks (1: Kamino+Jupiter, 2: Kamino+Phoenix, 3: Phoenix) |
| `wind_commit` | `venue_data: Vec<u8>` | keeper | registry, vault |
| `wind_abort` | – | keeper | registry, vault |
| `unwind_start` | `reason: u8` | keeper or guardian (emergency only) | registry, vault |
| `unwind_step` | `n: u8, venue_data: Vec<u8>` | keeper | registry, vault + venue blocks (1: Phoenix, 2: Phoenix+Kamino, 3: Kamino+Jupiter) |
| `unwind_commit` | `venue_data: Vec<u8>` | keeper | registry, vault + Kamino block |
| `unwind_partial_start` | `fraction_bps: u32, reason: u8` | keeper (or guardian, emergency) | registry, vault. Basis → PartialUnwinding(0) |
| `unwind_partial_step` | `n: u8, fraction_bps: u32, venue_data: Vec<u8>` | keeper | registry, vault + venue blocks (1: Phoenix, 2: Phoenix+Kamino, 3: Kamino+Jupiter). The fraction is repeated on every step; step 3's proceeds repay `fraction` of the primary loan |
| `unwind_partial_commit` | `venue_data: Vec<u8>` | keeper | registry, vault + Kamino block. Supplies the transit USDC, checks hedge/LTV/margin → Basis |
| `unwind_partial_abort` | – | keeper or guardian | registry, vault. PartialUnwinding → Unwinding(0) |
| `size_up_start` | `venue_data: Vec<u8>` | keeper | registry, vault + Kamino block. Parked: borrow + supply, stays Parked. Basis: borrow into transit → SizingUp(0) |
| `size_up_step` | `n: u8, venue_data: Vec<u8>` | keeper | registry, vault + venue blocks, exactly as `wind_step` |
| `size_up_commit` | `venue_data: Vec<u8>` | keeper | registry, vault → Basis |
| `size_up_abort` | – | keeper | registry, vault. SizingUp → Unwinding(0) |
| `rebalance_to_kamino` / `rebalance_to_phoenix` | `venue_data: Vec<u8>` | keeper | registry, vault + Kamino and Phoenix blocks |
| `rebalance_from_parked` | `amount: u64, venue_data: Vec<u8>` | keeper | registry, vault + Kamino block |
| `close_epoch` | – | keeper (payer) | registry, vault, exit_epoch (init_if_needed), system_program. Refused (`EpochNotSettled`) while an earlier epoch is closed but unsettled (`pending_exit_shares ≠ this epoch's shares`) |
| `settle_epoch` | `venue_data: Vec<u8>` | keeper | registry, vault, exit_epoch, stock_custody, usdc_buffer, redeem_stock, redeem_usdc, token_program, xstock_mint, stock_token_program + Kamino (and Phoenix in Basis) blocks. The USDC leg comes from `usdc_buffer` first (keeping 0.05 USDC unless nothing else can fund the epoch), then Kamino supply / free Phoenix collateral; the stock leg from custody first, then the obligation |
| `crystallise_fee` | – | keeper | registry, vault, share_mint, treasury_shares, token_program |

`mock_*` args are only honoured when the program is built with `mock-venues`; otherwise the value is
read from the venue account and the arg must be `None`. Exception (DECISIONS D6): `record_funding`'s
value is keeper-supplied on every build. `venue_data` is an empty `Vec` on mock builds.

## Venue blocks (non-mock builds)

`venue_data` is a Borsh `VenueData`:

```
VenueData { blocks: u8, phoenix_gti: u8, phoenix_atb: u8, base_lot_size: u64, price_in_ticks: u64,
            last_valid_slot: u64, phoenix_equity_usdc: u64, client_order_id: u64, jupiter_data: Vec<u8> }
// blocks bitmask: 1 = Kamino, 2 = Phoenix, 4 = Jupiter. Blocks appear in `remaining_accounts` in that order.
```

**Kamino block (24)**: klend_program, lending_market, lending_market_authority (`["lma", market]`), obligation
(`[0, 0, vault, market, system, system]`), stock_reserve, stock_reserve_liquidity_supply, stock_reserve_collateral_mint,
stock_reserve_collateral_supply, xstock_mint, usdc_reserve, usdc_reserve_liquidity_supply, usdc_reserve_fee_receiver,
usdc_reserve_collateral_mint, vault_usdc_ctoken (vault ATA of the USDC cToken), usdc_mint, scope_prices, farms_program,
instructions_sysvar, token_program, stock_token_program, stock_custody, usdc_buffer, usdc_debt_farm_state
(`reserve_usdc.farm_debt`, klend program id when none), obligation_debt_farm_user_state (`["user", farm_debt, obligation]`
of the Farms program, klend program id when none). The program checks every reserve-derived address against the
reserve account bytes. The mainnet USDC reserve has a debt farm; the xStocks reserves have no farms.

**Phoenix block (17 + gti + atb)**: phoenix_program, log_authority, global_configuration, trader_account, perp_asset_map,
orderbook, spline_collection, global_vault, trader_phoenix_token_account, canonical_mint, ember_program, ember_state,
ember_vault, usdc_mint, usdc_buffer, token_program, withdraw_queue (exchange-wide, used by `withdraw_funds`), then
`phoenix_gti` global-trader-index accounts and `phoenix_atb` active-trader-buffer accounts. Orders are all-or-nothing IOC with `min_base_lots_to_fill = num_base_lots`; `base_lot_size`,
`phoenix_equity_usdc` and the funding rate are keeper-supplied (D6).

**Which blocks each instruction takes** (the keeper attaches only these; mainnet allows 64 account locks
per transaction and Kamino + Phoenix + a route is ~60–85 unique accounts): `init_kamino_obligation`,
`sync_collateral`, `wind_start`, `park`, `repay`, `rebalance_from_parked`, `unwind_commit` → Kamino;
`wind_step(1)`, `unwind_step(3)` → Kamino + Jupiter; `wind_step(2)`, `unwind_step(2)`, `settle_epoch`,
`rebalance_to_kamino`, `rebalance_to_phoenix` → Kamino + Phoenix; `wind_step(3)`, `unwind_step(1)` → Phoenix;
`wind_commit`, `size_up_commit` → none (equity is in `VenueData`); `size_up_step(n)` as `wind_step(n)`;
`unwind_partial_step(n)` as `unwind_step(n)`; `unwind_partial_commit` → Kamino. No instruction takes all
three blocks (the keeper still refuses any transaction over 64 unique accounts before signing).
Commits supply back only the transit USDC (`usdc_buffer` minus a 0.05 USDC cushion, capped by
`parked_usdc`), so a parked leg that is already supplied is never supplied twice. `unwind_partial_commit`
and `settle_epoch` accept an LTV up to `ltv_bps + rebalance_ltv_band_bps` (a proportional release leaves
the LTV at L plus rounding); the fast loop's `rebalance_to_kamino` brings it back to L.
Global configuration and the canonical mint are writable in the Phoenix block (Phoenix and Ember write them).

**Jupiter block (last)**: jupiter_program, then the `shared_accounts_route` accounts exactly as the swap-instructions API
returned them with the vault as `user_transfer_authority` (index 2) and the vault's `usdc_buffer`/`stock_custody`
substituted at indices 3 and 6. `jupiter_data` is the API's instruction data; the program patches `in_amount`,
`quoted_out_amount` (= its oracle floor) and `slippage_bps` (= 0), and enforces the floor on the destination balance delta.
Transactions carrying a route must be v0 with the API's lookup tables.

Constant `DEBT_DUST_USDC = 10_000` (0.01 USDC): debt below it never blocks settlement and is written off on repay.
`repay` clears the whole loan through Kamino's repay-all (`u64::MAX`) whenever `usdc_buffer` plus the dust tolerance
covers it (Kamino will not leave a sub-minimum residual), so the keeper keeps a small USDC cushion in `usdc_buffer`.

## Token programs

xStock mints on mainnet are **Token-2022** (`TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb`, 8 decimals) with these
extensions: metadata pointer + token metadata, permanent delegate (issuer), default account state
(initialized), scaled UI amount (multiplier 1), pausable, confidential transfer (opt-in, auto-approve off),
transfer hook with **no hook program set**. USDC and the share mints are classic SPL Token
(`TokenkegQfeZYiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA`).

Every instruction that moves stock takes two token programs: `token_program` (classic, for shares and
USDC) and `stock_token_program` (the program owning `xstock_mint`; Token-2022 on mainnet). Stock
transfers use `transfer_checked` with the mint's decimals, so `xstock_mint` is an account on
`deposit`, `redeem` and `settle_epoch`. Stock token accounts (`stock_custody`, `redeem_stock`,
`user_stock`) must be owned by `stock_token_program`. If Backed ever sets a transfer hook program,
the hook's extra accounts must be appended as remaining accounts and the program updated to resolve
them; today none is set.

## Events

Field order is the Borsh order.

`Deposited{vault, user, qty, shares}`,
`ExitRequested{vault, user, nonce, shares, epoch_id}`,
`ExitCancelled{vault, user, nonce, shares}`,
`EpochClosed{vault, epoch_id, shares_total, stock_owed, usdc_owed}`,
`EpochSettled{vault, epoch_id, stock_paid, usdc_paid}`,
`Redeemed{vault, user, nonce, shares, stock, usdc}`,
`StateChanged{vault, from, to, step}`,
`RuleEvaluated{vault, f_avg_bps, parked_apy_bps, r_bps, hurdle_bps, decision, f_3h_bps, be_bps}` (D8: `hurdle_bps` = `be_bps` = r + L·r; `f_3h_bps` is the mean of the newest 3 samples, 0 with fewer),
`NavRefreshed{vault, nav_usd_e6, share_price_stock_e6, price_e6}`,
`FundingRecorded{vault, rate_bps_e6_hourly, f_avg_bps, samples}`,
`KaminoRatesRecorded{borrow_apy_bps, supply_apy_bps}`,
`Rebalanced{vault, kind, amount}` (kind: 0 to_kamino, 1 to_phoenix, 2 from_parked),
`FeeCrystallised{vault, shares, high_water_e6}`, `Paused{}`, `Unpaused{}`.

## Errors

`Paused`, `Unauthorized`, `WrongState`, `WrongStep`, `MarketClosed`, `RuleNotSatisfied`,
`InsufficientSamples`, `NavStale`, `SlippageExceeded`, `HedgeOutOfTolerance`, `LtvTooHigh`,
`MarginTooLow`, `DepositCapExceeded`, `EpochNotClosed`, `EpochNotSettled`, `EpochUnderfunded`,
`TooSoon`, `VenueNotWired`, `MockNotAllowed`, `MathOverflow`, `InvalidArgument`, `VenueAccountsMissing`, `VenueAccountsMismatch`, `VenueCpiFailed`.
