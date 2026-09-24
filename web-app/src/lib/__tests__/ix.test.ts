import { describe, expect, it } from "vitest";
import { Keypair } from "@solana/web3.js";
import { depositIx, discriminator, redeemIx, requestExitIx, vaultKeys } from "@/lib/chain/ix";
import { PROGRAM_ID, USDC_MINT } from "@/lib/chain/config";
import { decodeOverlayVault, OverlayVaultSchema } from "@/lib/chain/layout";
import { serialize } from "borsh";

describe("discriminator", () => {
  it("is sha256('global:<name>')[0..8], matching Anchor", () => {
    // Known Anchor value: sha256("global:initialize") starts with af af 6d 1f 0d 98 9b ed
    expect(Buffer.from(discriminator("initialize")).toString("hex")).toBe("afaf6d1f0d989bed");
    expect(discriminator("deposit")).toHaveLength(8);
    expect(discriminator("deposit")).not.toEqual(discriminator("redeem"));
  });
});

describe("instruction builders", () => {
  const user = Keypair.generate().publicKey;
  const mint = Keypair.generate().publicKey;
  const k = vaultKeys(mint);

  it("deposit encodes discriminator + qty + min_shares (u64 LE) with 9 accounts", () => {
    const ix = depositIx(user, k, 5_000_000n, 1n);
    expect(ix.programId.equals(PROGRAM_ID)).toBe(true);
    expect(ix.keys).toHaveLength(9);
    expect(ix.keys[0].isSigner).toBe(true);
    expect(ix.data.subarray(0, 8)).toEqual(Buffer.from(discriminator("deposit")));
    expect(ix.data.readBigUInt64LE(8)).toBe(5_000_000n);
    expect(ix.data.readBigUInt64LE(16)).toBe(1n);
    expect(ix.data).toHaveLength(24);
  });

  it("request_exit derives the exit PDA from vault, user and nonce", () => {
    const a = requestExitIx(user, k, 10n, 1n, 0n);
    const b = requestExitIx(user, k, 10n, 2n, 0n);
    expect(a.keys[3].pubkey.equals(b.keys[3].pubkey)).toBe(false);
    expect(a.data).toHaveLength(24);
  });

  it("redeem has no args", () => {
    const ix = redeemIx(user, k, USDC_MINT, 1n, 0n);
    expect(ix.data).toHaveLength(8);
    expect(ix.keys).toHaveLength(11);
  });
});

describe("OverlayVault layout", () => {
  it("round-trips through the Borsh schema behind an 8-byte discriminator", () => {
    const params = {
      ltv_bps: 3000, min_margin_bps: 1200, liq_ltv_bps: 6500, emergency_ltv_bps: 6000, enter_margin_bps: 200, exit_margin_bps: 100,
      carry_guard_margin_bps: 50, min_enter_funding_bps: 450, expected_hold_hours: 720, roundtrip_cost_bps: 60, funding_window: 24,
      max_swap_slippage_bps: 50, max_perp_slippage_bps: 30, max_index_dev_bps: 50, hedge_tol_bps: 50, size_band_bps: 300,
      rebalance_ltv_band_bps: 800, rebalance_margin_band_bps: 500, max_nav_age_slots: 150n, epoch_len_secs: 3600n, perf_fee_bps: 1500,
      exit_fee_bps: 10, deposit_cap_stock: 1_000_000_000n, basis_cap_usdc: 25_000_000_000n,
    };
    const raw = {
      xstock_mint: Array(32).fill(1), share_mint: Array(32).fill(2), tier: 1, params, state: 3, step: 0, market_open: true,
      collateral_qty: 100n, basis_spot_qty: 30n, debt_usdc: 12_000n, debt_b_usdc: 3_600n, parked_usdc: 0n, phoenix_equity_usdc: 3_600n,
      phoenix_short_qty: 30n, funding: Array(24).fill(4n), funding_head: 3, funding_samples: 24, last_funding_ts: 1n,
      nav_usd_e6: 41_200_000_000n, share_price_stock_e6: 1_002_000n, price_e6: 412_000_000n, nav_slot: 10n, high_water_e6: 1_000_000n,
      total_shares: 70n, pending_exit_shares: 0n, epoch_id: 2n, epoch_opened_ts: 0n,
      last_rule: { f_avg_bps: 3500n, parked_apy_bps: 480, r_bps: 590, hurdle_bps: 1910n, decision: 1, ts: 5n },
      bump: 255,
    };
    const body = serialize(OverlayVaultSchema, raw);
    const data = new Uint8Array(8 + body.length);
    data.set(body, 8);
    const v = decodeOverlayVault(data);
    expect(v.state).toBe(3);
    expect(v.params.ltv_bps).toBe(3000);
    expect(v.priceE6).toBe(412_000_000n);
    expect(v.lastRule.hurdleBps).toBe(1910n);
    expect(v.funding).toHaveLength(24);
  });
});
