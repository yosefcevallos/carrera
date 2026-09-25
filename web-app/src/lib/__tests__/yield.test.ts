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

describe("estimatedYield", () => {
  const base = { mode: "funding" as const, marketOpen: true, priceUsd: 400, tvlUsd: 0, capUsd: 0, totalShares: 0, fundingAvgBps: 3500, hurdleBps: 1900, enterMarginBps: 200, exitMarginBps: 100, funding24h: [], ageDays: 0, usdcPerShare: 0, sharePriceHistory: [], trailing: { d7Bps: 0, d30Bps: 0, inceptionBps: 0, inceptionDays: 0 } };
  it("uses the rule's net carry when there is no realised history", async () => {
    const { estimatedYield } = await import("@/lib/yield");
    // L=0.30, f=35%, r=5.9%: 0.3·35 − 0.3·1.3·5.9 = 10.5 − 2.301 = 8.199
    expect(estimatedYield(base, 3000, 590, 480)).toBeCloseTo(8.2, 1);
    expect(estimatedYield({ ...base, mode: "idle" }, 3000, 590, 480)).toBe(0);
    expect(estimatedYield({ ...base, mode: "parked" }, 3000, 590, 650)).toBeCloseTo(0.18, 2);
  });
  it("prefers realised 30d growth when present", async () => {
    const { estimatedYield } = await import("@/lib/yield");
    const v = { ...base, trailing: { ...base.trailing, d30Bps: 50 } }; // 0.5% in 30d → 6.08% a year
    expect(estimatedYield(v, 3000, 590, 480)).toBeCloseTo(6.08, 1);
  });
});
