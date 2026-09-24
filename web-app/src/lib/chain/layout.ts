// Borsh layouts for Registry and OverlayVault per docs/CONTRACT.md (field order is the Borsh order).
import { deserialize, type Schema } from "borsh";
import { PublicKey } from "@solana/web3.js";

const pubkey: Schema = { array: { type: "u8", len: 32 } };

export const VaultParamsSchema: Schema = {
  struct: {
    ltv_bps: "u32",
    min_margin_bps: "u32",
    liq_ltv_bps: "u32",
    emergency_ltv_bps: "u32",
    enter_margin_bps: "u32",
    exit_margin_bps: "u32",
    carry_guard_margin_bps: "u32",
    min_enter_funding_bps: "u32",
    expected_hold_hours: "u32",
    roundtrip_cost_bps: "u32",
    funding_window: "u8",
    max_swap_slippage_bps: "u32",
    max_perp_slippage_bps: "u32",
    max_index_dev_bps: "u32",
    hedge_tol_bps: "u32",
    size_band_bps: "u32",
    rebalance_ltv_band_bps: "u32",
    rebalance_margin_band_bps: "u32",
    max_nav_age_slots: "u64",
    epoch_len_secs: "u64",
    perf_fee_bps: "u32",
    exit_fee_bps: "u32",
    deposit_cap_stock: "u64",
    basis_cap_usdc: "u64",
  },
};

export const RuleEvaluationSchema: Schema = {
  struct: { f_avg_bps: "i64", parked_apy_bps: "u32", r_bps: "u32", hurdle_bps: "i64", decision: "u8", ts: "i64" },
};

export const RegistrySchema: Schema = {
  struct: {
    admin: pubkey,
    guardian: pubkey,
    keepers: { array: { type: pubkey, len: 4 } },
    keeper_count: "u8",
    paused: "bool",
    usdc_mint: pubkey,
    borrow_apy_bps: "u32",
    supply_apy_bps: "u32",
    rates_slot: "u64",
    bump: "u8",
  },
};

export const OverlayVaultSchema: Schema = {
  struct: {
    xstock_mint: pubkey,
    share_mint: pubkey,
    tier: "u8",
    params: VaultParamsSchema,
    state: "u8",
    step: "u8",
    market_open: "bool",
    collateral_qty: "u64",
    basis_spot_qty: "u64",
    debt_usdc: "u64",
    debt_b_usdc: "u64",
    parked_usdc: "u64",
    phoenix_equity_usdc: "u64",
    phoenix_short_qty: "u64",
    funding: { array: { type: "i64", len: 24 } },
    funding_head: "u8",
    funding_samples: "u8",
    last_funding_ts: "i64",
    nav_usd_e6: "u64",
    share_price_stock_e6: "u64",
    price_e6: "u64",
    nav_slot: "u64",
    high_water_e6: "u64",
    total_shares: "u64",
    pending_exit_shares: "u64",
    epoch_id: "u64",
    epoch_opened_ts: "i64",
    last_rule: RuleEvaluationSchema,
    bump: "u8",
  },
};

export interface VaultParams {
  ltv_bps: number;
  min_margin_bps: number;
  liq_ltv_bps: number;
  emergency_ltv_bps: number;
  enter_margin_bps: number;
  exit_margin_bps: number;
  carry_guard_margin_bps: number;
  min_enter_funding_bps: number;
  expected_hold_hours: number;
  roundtrip_cost_bps: number;
  funding_window: number;
  max_swap_slippage_bps: number;
  max_perp_slippage_bps: number;
  max_index_dev_bps: number;
  hedge_tol_bps: number;
  size_band_bps: number;
  rebalance_ltv_band_bps: number;
  rebalance_margin_band_bps: number;
  max_nav_age_slots: bigint;
  epoch_len_secs: bigint;
  perf_fee_bps: number;
  exit_fee_bps: number;
  deposit_cap_stock: bigint;
  basis_cap_usdc: bigint;
}

export interface RegistryAccount {
  admin: PublicKey;
  guardian: PublicKey;
  keepers: PublicKey[];
  paused: boolean;
  usdcMint: PublicKey;
  borrowApyBps: number;
  supplyApyBps: number;
  ratesSlot: bigint;
}

export interface OverlayVaultAccount {
  xstockMint: PublicKey;
  shareMint: PublicKey;
  tier: number;
  params: VaultParams;
  state: number;
  step: number;
  marketOpen: boolean;
  collateralQty: bigint;
  basisSpotQty: bigint;
  debtUsdc: bigint;
  debtBUsdc: bigint;
  parkedUsdc: bigint;
  phoenixEquityUsdc: bigint;
  phoenixShortQty: bigint;
  funding: bigint[];
  fundingHead: number;
  fundingSamples: number;
  lastFundingTs: bigint;
  navUsdE6: bigint;
  sharePriceStockE6: bigint;
  priceE6: bigint;
  navSlot: bigint;
  highWaterE6: bigint;
  totalShares: bigint;
  pendingExitShares: bigint;
  epochId: bigint;
  epochOpenedTs: bigint;
  lastRule: { fAvgBps: bigint; parkedApyBps: number; rBps: number; hurdleBps: bigint; decision: number; ts: bigint };
}

export const VaultState = { Idle: 0, Parked: 1, Winding: 2, Basis: 3, Unwinding: 4 } as const;

const DISC_LEN = 8;
const pk = (a: number[]) => new PublicKey(Uint8Array.from(a));

/* eslint-disable @typescript-eslint/no-explicit-any */
export function decodeRegistry(data: Uint8Array): RegistryAccount {
  const r: any = deserialize(RegistrySchema, data.subarray(DISC_LEN));
  return {
    admin: pk(r.admin),
    guardian: pk(r.guardian),
    keepers: (r.keepers as number[][]).slice(0, r.keeper_count).map(pk),
    paused: r.paused,
    usdcMint: pk(r.usdc_mint),
    borrowApyBps: r.borrow_apy_bps,
    supplyApyBps: r.supply_apy_bps,
    ratesSlot: r.rates_slot,
  };
}

export function decodeOverlayVault(data: Uint8Array): OverlayVaultAccount {
  const v: any = deserialize(OverlayVaultSchema, data.subarray(DISC_LEN));
  return {
    xstockMint: pk(v.xstock_mint),
    shareMint: pk(v.share_mint),
    tier: v.tier,
    params: v.params,
    state: v.state,
    step: v.step,
    marketOpen: v.market_open,
    collateralQty: v.collateral_qty,
    basisSpotQty: v.basis_spot_qty,
    debtUsdc: v.debt_usdc,
    debtBUsdc: v.debt_b_usdc,
    parkedUsdc: v.parked_usdc,
    phoenixEquityUsdc: v.phoenix_equity_usdc,
    phoenixShortQty: v.phoenix_short_qty,
    funding: v.funding,
    fundingHead: v.funding_head,
    fundingSamples: v.funding_samples,
    lastFundingTs: v.last_funding_ts,
    navUsdE6: v.nav_usd_e6,
    sharePriceStockE6: v.share_price_stock_e6,
    priceE6: v.price_e6,
    navSlot: v.nav_slot,
    highWaterE6: v.high_water_e6,
    totalShares: v.total_shares,
    pendingExitShares: v.pending_exit_shares,
    epochId: v.epoch_id,
    epochOpenedTs: v.epoch_opened_ts,
    lastRule: {
      fAvgBps: v.last_rule.f_avg_bps,
      parkedApyBps: v.last_rule.parked_apy_bps,
      rBps: v.last_rule.r_bps,
      hurdleBps: v.last_rule.hurdle_bps,
      decision: v.last_rule.decision,
      ts: v.last_rule.ts,
    },
  };
}
/* eslint-enable @typescript-eslint/no-explicit-any */
