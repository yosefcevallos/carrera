import { describe, expect, it } from "vitest";
import type { SharePricePoint } from "@/lib/types";
import { bands, redemptionPreview, trailingYield } from "@/lib/yield";

const series = (n: number, perDay: number): SharePricePoint[] =>
  Array.from({ length: n + 1 }, (_, i) => ({ date: `2026-09-${String(i + 1).padStart(2, "0")}`, usdcPerShare: i * perDay, mode: "funding" }));

describe("trailingYield", () => {
  it("annualises USDC gained on stock value over the window", () => {
    // 0.10 USDC/day on a $100 stock over 30 days = 3 USDC → 3% × 365/30 = 36.5%
    const y = trailingYield(series(30, 0.1), 30, 100);
    expect(y.days).toBe(30);
    expect(y.usdcGained).toBeCloseTo(3);
    expect(y.apy).toBeCloseTo(36.5);
    expect(y.sinceInception).toBe(false);
  });

  it("falls back to since-inception when the vault is younger than the window", () => {
    const y = trailingYield(series(4, 0.1), 7, 100);
    expect(y.days).toBe(4);
    expect(y.sinceInception).toBe(true);
    expect(y.apy).toBeCloseTo((0.4 / 100) * (365 / 4) * 100);
  });

  it("is zero with no history", () => {
    expect(trailingYield([], 30, 100)).toEqual({ apy: 0, usdcGained: 0, days: 0, sinceInception: true });
  });
});

describe("bands", () => {
  it("adds and subtracts the hysteresis margins", () => {
    expect(bands(1910, 200, 100)).toEqual({ enterBps: 2110, exitBps: 1810 });
  });
});

describe("redemptionPreview", () => {
  it("pays stock plus USDC after the fee", () => {
    const r = redemptionPreview(10, 20, 400, 10);
    expect(r.stockOut).toBe(10);
    expect(r.usdcOut).toBeCloseTo(19.98);
    expect(r.stockReduced).toBe(false);
  });
  it("reduces the stock leg when USDC is negative (spec §4.2)", () => {
    const r = redemptionPreview(10, -4, 400, 10);
    expect(r.usdcOut).toBe(0);
    expect(r.stockOut).toBeCloseTo(10 - 4 / 400);
    expect(r.stockReduced).toBe(true);
  });
});

describe("currentApyBps", () => {
  it("matches spec Part A on the live mainnet numbers", async () => {
    const { currentApyBps } = await import("@/lib/yield");
    // MSTR: L=20%, f_avg 32.51%, r 5.89% → 650 − 141 ≈ 509 bps
    expect(currentApyBps({ vaultState: 3, ltvBps: 2000, fundingAvgBps: 3251, borrowApyBps: 589, supplyApyBps: 478 })).toBe(509);
    // TSLA: L=30%, f_avg 28.82% → 865 − 230 ≈ 635 bps
    expect(currentApyBps({ vaultState: 3, ltvBps: 3000, fundingAvgBps: 2882, borrowApyBps: 589, supplyApyBps: 478 })).toBe(635);
    // Parked: L·(s − r); Idle, Winding, Unwinding: 0
    expect(currentApyBps({ vaultState: 1, ltvBps: 3000, fundingAvgBps: 2882, borrowApyBps: 589, supplyApyBps: 650 })).toBe(18);
    for (const st of [0, 2, 4]) expect(currentApyBps({ vaultState: st, ltvBps: 3000, fundingAvgBps: 2882, borrowApyBps: 589, supplyApyBps: 478 })).toBe(0);
  });
});
