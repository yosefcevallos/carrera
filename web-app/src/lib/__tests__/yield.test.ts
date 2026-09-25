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

describe("FundingWave daily grouping", () => {
  it("averages each UTC day, keeps the newest seven, marks today as partial", async () => {
    const { byDay, dayLabel, countLabel } = await import("@/components/FundingWave");
    const day0 = Date.UTC(2026, 8, 16); // Wed 16 Sep, UTC midnight
    // 8 full days of 24 samples (rate = day index × 100), then 9 samples of a ninth, partial day.
    const samples = [];
    for (let d = 0; d < 8; d++) for (let h = 0; h < 24; h++) samples.push({ ts: day0 + d * 86_400_000 + h * 3_600_000, rateScaled: (d + 1) * 100 });
    for (let h = 0; h < 9; h++) samples.push({ ts: day0 + 8 * 86_400_000 + h * 3_600_000, rateScaled: -300 });
    const now = day0 + 8 * 86_400_000 + 9 * 3_600_000;
    const bars = byDay(samples, 7, now);
    expect(bars).toHaveLength(7); // oldest two days dropped
    expect(bars.map((b) => b.rateScaled)).toEqual([300, 400, 500, 600, 700, 800, -300]);
    expect(bars.map((b) => b.count)).toEqual([24, 24, 24, 24, 24, 24, 9]);
    expect(bars.map((b) => b.partial)).toEqual([false, false, false, false, false, false, true]);
    expect(dayLabel(bars[0])).toBe("Fri"); // 18 Sep 2026 is a Friday
    expect(dayLabel(bars[6])).toBe("Today so far");
    expect(countLabel(bars[0])).toBe("24 hourly samples");
    expect(countLabel(bars[6])).toBe("9 so far");
    expect(byDay([], 7, now)).toEqual([]);
  });
});
