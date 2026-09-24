import { describe, expect, it } from "vitest";
import { snapshotRows, type KeeperStatus } from "../src/sources/keeper.js";
import { extractLogs } from "../src/sources/helius.js";

const status: KeeperStatus = {
  keeper: { instance_id: "k1", is_leader: true, last_hourly_run_ts: 1, last_fast_run_ts: 2, sol_balance: 1.5, alerts: [] },
  vaults: [{
    symbol: "TSLA", state: "basis", step: 0, ltv_bps: 2900, margin_bps: 1500,
    net_delta: { qty: 4, usd_e6: 1_650_000 }, carry: { accrued_usdc_e6: 2_140_000_000, ann_net_bps: 860, estimated: true },
    legs: [{ kind: "long_spot" }], nav_usd_e6: 1, share_price_stock_e6: 1_000_000, nav_slot: 10,
  }],
};

describe("keeper snapshot rows", () => {
  it("maps one row per vault with the snapshot timestamp", () => {
    const rows = snapshotRows(status, new Date("2026-09-24T18:00:00Z"));
    expect(rows).toEqual([{
      vault_symbol: "TSLA", ts: "2026-09-24T18:00:00.000Z", state: "basis", step: 0, ltv_bps: 2900, margin_bps: 1500,
      net_delta_qty: 4, carry_accrued_usdc_e6: 2_140_000_000, carry_ann_net_bps: 860, legs: [{ kind: "long_spot" }],
    }]);
  });
});

describe("helius payload extraction", () => {
  it("uses raw logMessages when present", () => {
    const out = extractLogs({ transaction: { signatures: ["sig"] }, slot: 5, timestamp: 100, meta: { logMessages: ["a"] } });
    expect(out).toEqual({ signature: "sig", slot: 5, blockTime: new Date(100_000), logs: ["a"] });
  });
  it("returns null for enhanced payloads without logs (caller refetches) and for failed txs", () => {
    expect(extractLogs({ signature: "sig", slot: 1 })).toBeNull();
    expect(extractLogs({ signature: "sig", meta: { err: {}, logMessages: [] } })).toBeNull();
  });
});
