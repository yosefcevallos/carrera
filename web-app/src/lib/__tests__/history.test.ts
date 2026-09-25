import { describe, expect, it } from "vitest";
import { TICKERS } from "@/constants/vaults";
import { mapExits, mapHistory, type HistoryRows } from "@/lib/history";
import { zeroed } from "@/lib/zeroed";
import { createVaultStore } from "@/store/vault-store";

const prices = { ...zeroed(TICKERS), TSLA: 400, NVDA: 180 };
const NOW = Date.parse("2026-09-24T12:00:00Z");

describe("mapHistory", () => {
  it("fills every ticker when no view returns a row", () => {
    const snap = mapHistory({ nav: [], rules: [], trailing: [], funding: [], protocol: [] }, prices, NOW);
    for (const t of TICKERS) {
      expect(snap.vaults[t]).toEqual({ sharePriceHistory: [], funding24h: [], trailing: { d7Bps: 0, d30Bps: 0, inceptionBps: 0, inceptionDays: 0 }, ageDays: 0 });
    }
    expect(snap.usdcPaid24h).toBe(0);
    expect(snap.depositors).toBe(0);
  });

  it("fills every ticker when only one vault has rows, and downsamples nav to one point per day", () => {
    const rows: HistoryRows = {
      nav: [
        { vault_symbol: "TSLA", ts: "2026-09-22T09:00:00Z", share_price_stock_e6: 1_000_000, price_e6: null },
        { vault_symbol: "TSLA", ts: "2026-09-22T18:00:00Z", share_price_stock_e6: 1_001_000, price_e6: 400_000_000 },
        { vault_symbol: "TSLA", ts: "2026-09-23T09:00:00Z", share_price_stock_e6: 1_002_500, price_e6: 410_000_000 },
        { vault_symbol: "ZZZZ", ts: "2026-09-23T09:00:00Z", share_price_stock_e6: 1, price_e6: null },
      ],
      rules: [
        { vault_symbol: "TSLA", ts: "2026-09-22T10:00:00Z", state: 1 },
        { vault_symbol: "TSLA", ts: "2026-09-23T10:00:00Z", state: 3 },
      ],
      trailing: [{ vault_symbol: "TSLA", inception_at: "2026-09-10T12:00:00Z", growth_7d_bps: 12, growth_30d_bps: null, growth_inception_bps: "25" }],
      funding: [
        { vault_symbol: "TSLA", ts: "2026-09-24T11:00:00Z", rate_hourly_scaled: 400_000 }, // 0.4 bps/h → 35.04% a year
        { vault_symbol: "TSLA", ts: "2026-09-24T10:00:00Z", rate_hourly_scaled: 200_000 },
      ],
      protocol: [{ usdc_paid_24h: 1_062_000_000, depositors: 1284 }],
    };
    const snap = mapHistory(rows, prices, NOW);
    for (const t of TICKERS) expect(snap.vaults[t]).toBeDefined();
    expect(snap.vaults.NVDA.sharePriceHistory).toEqual([]);
    const h = snap.vaults.TSLA.sharePriceHistory;
    expect(h).toHaveLength(2);
    expect(h[0].date).toBe("2026-09-22");
    expect(h[0].usdcPerShare).toBeCloseTo(0.4); // last sample of the day, priced at 400
    expect(h[0].mode).toBe("parked");
    expect(h[1].date).toBe("2026-09-23");
    expect(h[1].usdcPerShare).toBeCloseTo(0.0025 * 410);
    expect(h[1].mode).toBe("funding");
    expect(snap.vaults.TSLA.trailing).toEqual({ d7Bps: 12, d30Bps: 0, inceptionBps: 25, inceptionDays: 14 });
    expect(snap.vaults.TSLA.ageDays).toBe(14);
    expect(snap.vaults.TSLA.funding24h.map((v) => +v.toFixed(2))).toEqual([17.52, 35.04]); // oldest first
    expect(snap.usdcPaid24h).toBe(1062);
    expect(snap.depositors).toBe(1284);
  });

  it("merges into the vault store without changing its shape", () => {
    const store = createVaultStore();
    const before = Object.keys(store.getState().vaults).sort();
    const snap = mapHistory({ nav: [], rules: [], trailing: [], funding: [], protocol: [] }, prices, NOW);
    const updates = Object.fromEntries(TICKERS.map((t) => [t, { ...store.getState().vaults[t], ...snap.vaults[t] }]));
    store.getState().setVaults(updates);
    expect(Object.keys(store.getState().vaults).sort()).toEqual(before);
  });
});

describe("mapExits", () => {
  it("returns every ticker with zeros when there are no rows", () => {
    const out = mapExits([]);
    for (const t of TICKERS) expect(out[t]).toEqual({ shares: 0, stockAmount: 0, usdcAmount: 0, readyAt: 0, ready: false, nonce: 0 });
  });

  it("maps open and settled exits, ignoring redeemed and cancelled", () => {
    const out = mapExits([
      { vault_symbol: "TSLA", nonce: 7, shares: 400_000_000, epoch_id: 3, status: 1, requested_at: "2026-09-24T10:30:00Z", stock_out: 399_000_000, usdc_out: 21_430_000 },
      { vault_symbol: "NVDA", nonce: null, shares: 100_000_000, epoch_id: 3, status: 0, requested_at: "2026-09-24T10:30:00Z", stock_out: null, usdc_out: null },
      { vault_symbol: "CRCL", nonce: 1, shares: 100_000_000, epoch_id: 2, status: 2, requested_at: "2026-09-24T09:30:00Z", stock_out: 100_000_000, usdc_out: 0 },
    ]);
    expect(out.TSLA).toEqual({ shares: 4, stockAmount: 3.99, usdcAmount: 21.43, readyAt: Date.parse("2026-09-24T11:30:00Z"), ready: true, nonce: 7 });
    expect(out.NVDA).toMatchObject({ shares: 1, stockAmount: 1, usdcAmount: 0, ready: false, nonce: 0 });
    expect(out.CRCL.shares).toBe(0);
  });
});
