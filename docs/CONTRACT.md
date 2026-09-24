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
VaultState: Idle=0, Parked=1, Winding=2, Basis=3, Unwinding=4
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

RuleEvaluation { f_avg_bps: i64, parked_apy_bps: u32, r_bps: u32, hurdle_bps: i64, decision: u8, ts: i64 }
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
| `record_funding` | `mock_rate_bps_hourly: Option<i64>` | anyone | registry, vault, hawkeye_view (unchecked) |
| `record_kamino_rates` | `mock_borrow_bps: Option<u32>, mock_supply_bps: Option<u32>` | anyone | registry, kamino_reserve (unchecked) |
| `refresh_nav` | `mock_price_e6: Option<u64>` | anyone | registry, vault, oracle (unchecked) |
| `park` | – | keeper | registry, vault |
| `repay` | – | keeper or guardian | registry, vault |
| `wind_start` | – | keeper | registry, vault |
| `wind_step` | `n: u8` | keeper | registry, vault |
| `wind_commit` | – | keeper | registry, vault |
| `wind_abort` | – | keeper | registry, vault |
| `unwind_start` | `reason: u8` | keeper or guardian (emergency only) | registry, vault |
| `unwind_step` | `n: u8` | keeper | registry, vault |
| `unwind_commit` | – | keeper | registry, vault |
| `unwind_partial` | `fraction_bps: u32, reason: u8` | keeper | registry, vault |
| `size_up` | – | keeper | registry, vault |
| `rebalance_to_kamino` / `rebalance_to_phoenix` | – | keeper | registry, vault |
| `rebalance_from_parked` | `amount: u64` | keeper | registry, vault |
| `close_epoch` | – | keeper (payer) | registry, vault, exit_epoch (init_if_needed), system_program |
| `settle_epoch` | – | keeper | registry, vault, exit_epoch, stock_custody, usdc_buffer, redeem_stock, redeem_usdc, token_program, xstock_mint, stock_token_program |
| `crystallise_fee` | – | keeper | registry, vault, share_mint, treasury_shares, token_program |

`mock_*` args are only honoured when the program is built with `mock-venues`; otherwise
the value is read from the venue account and the arg must be `None`.

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
`RuleEvaluated{vault, f_avg_bps, parked_apy_bps, r_bps, hurdle_bps, decision}`,
`NavRefreshed{vault, nav_usd_e6, share_price_stock_e6, price_e6}`,
`FundingRecorded{vault, rate_bps_e6_hourly, f_avg_bps, samples}`,
`KaminoRatesRecorded{borrow_apy_bps, supply_apy_bps}`,
`Rebalanced{vault, kind, amount}` (kind: 0 to_kamino, 1 to_phoenix, 2 from_parked),
`FeeCrystallised{vault, shares, high_water_e6}`, `Paused{}`, `Unpaused{}`.

## Errors

`Paused`, `Unauthorized`, `WrongState`, `WrongStep`, `MarketClosed`, `RuleNotSatisfied`,
`InsufficientSamples`, `NavStale`, `SlippageExceeded`, `HedgeOutOfTolerance`, `LtvTooHigh`,
`MarginTooLow`, `DepositCapExceeded`, `EpochNotClosed`, `EpochNotSettled`, `EpochUnderfunded`,
`TooSoon`, `VenueNotWired`, `MockNotAllowed`, `MathOverflow`.
