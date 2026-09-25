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

describe("realisedGrowth / formatGrowth", () => {
  const base = { mode: "funding" as const, marketOpen: true, priceUsd: 400, tvlUsd: 0, capUsd: 0, totalShares: 0, fundingAvgBps: 0, hurdleBps: 0, vaultState: 3, ltvBps: 3000, borrowApyBps: 0, supplyApyBps: 0, enterMarginBps: 0, exitMarginBps: 0, fundingSamples: [], ageDays: 0, usdcPerShare: 0, sharePriceHistory: [], trailing: { d7Bps: 0, d30Bps: 0, inceptionBps: 0, inceptionDays: 0 } };

  it("a six-hour-old vault at share price 0.9993 shows −0.07% since inception, never an annualised figure", async () => {
    const { realisedGrowth, formatGrowth } = await import("@/lib/yield");
    // share price 0.9993 → inception growth −7 bps; inception 6h ago → 0 whole days
    const v = { ...base, trailing: { d7Bps: 0, d30Bps: 0, inceptionBps: -7, inceptionDays: 0 } };
    const g7 = realisedGrowth(v, 7);
    expect(g7.growthPct).toBeCloseTo(-0.07, 5);
    expect(g7.sinceInception).toBe(true);
    expect(formatGrowth(g7, 7)).toBe("−0.07% since inception");
    expect(formatGrowth(g7, 7)).not.toMatch(/a year|%.*\d{2,}\.\d%/);
    expect(Math.abs(g7.growthPct)).toBeLessThan(1); // −7 bps annualised would be ≈ −102%
  });

  it("formats a full window as raw growth and a young vault with its age", async () => {
    const { realisedGrowth, formatGrowth } = await import("@/lib/yield");
    const full = { ...base, trailing: { d7Bps: 2, d30Bps: 9, inceptionBps: 12, inceptionDays: 40 } };
    expect(formatGrowth(realisedGrowth(full, 7), 7)).toBe("+0.02% in 7d");
    expect(formatGrowth(realisedGrowth(full, 30), 30)).toBe("+0.09% in 30d");
    const young = { ...base, trailing: { d7Bps: 0, d30Bps: 0, inceptionBps: 1, inceptionDays: 1 } };
    expect(formatGrowth(realisedGrowth(young, 7), 7)).toBe("+0.01% since inception, 1d");
  });
});

describe("FundingWave window and geometry", () => {
  // Live v_funding_7d daily means for QQQ, annualised %: the oldest is a partial day outside the window.
  const QQQ = [-2.2, -10.3, -2.4, 2.7, 17.3, 37.2, 35.4, 52.1];
  const toScaled = (annualPct: number) => Math.round((annualPct * 100 * 1_000_000) / 8760); // % → bps → hourly scaled

  it("keeps exactly the 7 most recent UTC days including today and drops the older partial day", async () => {
    const { byDay } = await import("@/components/FundingWave");
    const today = Date.UTC(2026, 8, 25);
    const now = today + 3 * 3_600_000;
    const samples = QQQ.flatMap((pct, i) => {
      const day = today - (7 - i) * 86_400_000; // i=0 is 7 days ago (outside), i=7 is today
      const hours = i === 0 ? 5 : i === 7 ? 3 : 24;
      return Array.from({ length: hours }, (_, h) => ({ ts: day + h * 3_600_000, rateScaled: toScaled(pct) }));
    });
    const bars = byDay(samples, 7, now);
    expect(bars).toHaveLength(7);
    expect(bars[0].day).toBe(today - 6 * 86_400_000);
    expect(bars[6]).toMatchObject({ day: today, partial: true, count: 3 });
    expect(bars.map((b) => Math.sign(b.rateScaled))).toEqual([-1, -1, 1, 1, 1, 1, 1]);
  });

  it("puts the baseline mid-band when any day is negative and draws those bars downward, min 2px", async () => {
    const { layoutBars } = await import("@/components/FundingWave");
    const geo = layoutBars(QQQ.slice(1).map(toScaled), 30); // the 7 in-window days: two negative
    expect(geo.baselineY).toBe(15);
    const down = geo.rects.filter((r) => r.neg);
    expect(down).toHaveLength(2);
    for (const r of down) expect(r.y).toBe(15); // start at the baseline and extend down
    for (const r of geo.rects.filter((r) => !r.neg)) expect(r.y + r.height).toBeCloseTo(15);
    expect(Math.max(...geo.rects.map((r) => r.height))).toBeCloseTo(15); // max |value| fills its half
    expect(Math.min(...geo.rects.map((r) => r.height))).toBeGreaterThanOrEqual(2);
    // SPY has three negative-to-positive swings too; all-positive series keeps the baseline at the bottom
    const flat = layoutBars([1, 2, 3].map(toScaled), 30);
    expect(flat.baselineY).toBe(30);
    expect(flat.rects.every((r) => !r.neg && r.y + r.height === 30)).toBe(true);
    expect(layoutBars([0, 0, 0], 30).rects.every((r) => r.height === 2)).toBe(true);
  });
});
