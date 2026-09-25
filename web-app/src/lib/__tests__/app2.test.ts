import { describe, expect, it } from "vitest";
import { TICKERS, type Ticker } from "@/constants/vaults";
import { zeroVault } from "@/store/vault-store";
import { zeroPosition } from "@/store/position-store";
import { filled } from "@/lib/zeroed";
import {
  agoLabel, chipRaw, filterRows, formatCountdown, hourUtc, nextTopOfHour, sharedScale, smooth5,
  sortRows, sparkPath, sparkSeries, weightedApyBps, type RowInput,
} from "@/lib/app2";

function row(t: Ticker, over: Partial<RowInput["v"]> = {}, p: Partial<RowInput["p"]> = {}): RowInput {
  return { t, v: { ...zeroVault(), ...over }, p: { ...zeroPosition(), ...p }, exits: [] };
}

describe("sparkline", () => {
  it("smooths with a centred 5-point mean and keeps length", () => {
    const s = smooth5([0, 0, 10, 0, 0]);
    expect(s).toHaveLength(5);
    expect(s[2]).toBeCloseTo(2);
    expect(s[0]).toBeCloseTo(10 / 3);
  });
  it("uses one shared scale across vaults", () => {
    const a = sparkSeries(Array.from({ length: 30 }, (_, i) => ({ ts: i, rateScaled: 100 })));
    const b = sparkSeries(Array.from({ length: 30 }, (_, i) => ({ ts: i, rateScaled: -400 })));
    expect(a).toHaveLength(28);
    const max = sharedScale([a, b]);
    expect(max).toBe(400);
    const ga = sparkPath(a, max);
    const gb = sparkPath(b, max);
    expect(ga.last!.y).toBeLessThan(ga.midY); // positive above the zero line
    expect(gb.last!.y).toBeGreaterThan(gb.midY); // negative below
    expect(gb.last!.y).toBeCloseTo(22); // full scale reaches the 2 px margin
    expect(sparkPath([], max).path).toBe("");
  });
});

describe("table sort and filter", () => {
  const rows = [
    row("SPY", { vaultState: 0 }),
    row("TSLA", { vaultState: 3, ltvBps: 3000, fundingAvgBps: 2882, borrowApyBps: 589, mode: "funding", priceUsd: 380 }, { shares: 1, stockAmount: 0.0264 }),
    row("QQQ", { vaultState: 3, ltvBps: 3000, fundingAvgBps: 3330, borrowApyBps: 589, mode: "funding", priceUsd: 738 }, { shares: 1, stockAmount: 0.0134 }),
  ];
  it("sorts by APY desc by default and asc when flipped", () => {
    expect(sortRows(rows, "apy", "desc").map((r) => r.t)).toEqual(["QQQ", "TSLA", "SPY"]);
    expect(sortRows(rows, "apy", "asc").map((r) => r.t)).toEqual(["SPY", "TSLA", "QQQ"]);
  });
  it("sorts by asset name and by position value", () => {
    expect(sortRows(rows, "asset", "asc").map((r) => r.t)).toEqual(["QQQ", "SPY", "TSLA"]);
    expect(sortRows(rows, "position", "desc")[0].t).toBe("TSLA"); // 0.0264 × 380 > 0.0134 × 738
  });
  it("filters by segment and ticker prefix", () => {
    expect(filterRows(rows, "funding", "").map((r) => r.t).sort()).toEqual(["QQQ", "TSLA"]);
    expect(filterRows(rows, "positions", "").map((r) => r.t).sort()).toEqual(["QQQ", "TSLA"]);
    expect(filterRows(rows, "all", "ts").map((r) => r.t)).toEqual(["TSLA"]);
    expect(filterRows(rows, "all", "zzz")).toHaveLength(0);
  });
});

describe("chips and clocks", () => {
  it("fills percentage chips from raw units without exceeding the balance", () => {
    expect(chipRaw(1295716n, 25)).toBe(323929n);
    expect(chipRaw(1295716n, 50)).toBe(647858n);
    expect(chipRaw(1295716n, 100)).toBe(1295716n);
    expect(chipRaw(3n, 10)).toBe(0n);
  });
  it("counts down to the next top of the hour", () => {
    const now = Date.UTC(2026, 8, 25, 1, 40, 18);
    const next = nextTopOfHour(now);
    expect(new Date(next).toISOString()).toBe("2026-09-25T02:00:00.000Z");
    expect(formatCountdown(next - now)).toBe("19:42");
    expect(formatCountdown(-5)).toBe("00:00");
    expect(formatCountdown(3_725_000)).toBe("62:05");
  });
  it("labels the funding cycle hour in UTC and the sync age", () => {
    expect(hourUtc(Date.UTC(2026, 8, 24, 23, 17))).toBe("23:00 UTC");
    const now = Date.now();
    expect(agoLabel(0, now)).toBe("—");
    expect(agoLabel(now - 10_000, now)).toBe("just now");
    expect(agoLabel(now - 125_000, now)).toBe("2m ago");
    expect(agoLabel(now - 7_300_000, now)).toBe("2h ago");
  });
  it("weights the user's APY by position value", () => {
    const vaults = filled(TICKERS, zeroVault);
    const positions = filled(TICKERS, zeroPosition);
    vaults.TSLA = { ...zeroVault(), vaultState: 3, ltvBps: 3000, fundingAvgBps: 2882, borrowApyBps: 589, priceUsd: 100 };
    vaults.SPY = { ...zeroVault(), vaultState: 0, priceUsd: 100 };
    positions.TSLA = { shares: 1, stockAmount: 1, usdcEarned: 0 };
    positions.SPY = { shares: 1, stockAmount: 1, usdcEarned: 0 };
    // TSLA ≈ 635 bps on half the value, SPY 0 on the other half
    expect(weightedApyBps(vaults, positions)).toBe(318);
    expect(weightedApyBps(filled(TICKERS, zeroVault), filled(TICKERS, zeroPosition))).toBe(0);
  });
});

import { hourLabel, sparkRaw, sparkSummary } from "@/lib/app2";
import { annualisedPct } from "@/lib/yield";

describe("sparkline tooltip data", () => {
  it("keeps the raw hourly sample behind each smoothed point", () => {
    const samples = Array.from({ length: 40 }, (_, i) => ({ ts: i * 3_600_000, rateScaled: i === 30 ? 900_000 : 100_000 }));
    const raw = sparkRaw(samples);
    expect(raw).toHaveLength(28);
    expect(raw[raw.length - 1].ts).toBe(39 * 3_600_000);
    // index 30 of the source is index 18 of the 28-sample tail; the raw value is the spike, the smoothed one is not
    expect(raw[18].rateScaled).toBe(900_000);
    expect(annualisedPct(raw[18].rateScaled)).toBeCloseTo(78.84, 2);
    const summary = sparkSummary(raw, annualisedPct);
    expect(summary).toContain("8.8% to 78.8% a year");
    expect(summary).toContain("28 samples");
    expect(sparkSummary([], annualisedPct)).toBe("No funding samples yet");
  });
  it("labels the hour in local time with a weekday", () => {
    const label = hourLabel(Date.UTC(2026, 8, 24, 19, 0));
    expect(label).toMatch(/^[A-Z][a-z]{2} \d{1,2}(:\d{2})? ?(am|pm)?$/);
  });
});

import { fundingNowPct } from "@/lib/app2";

describe("funding now", () => {
  it("annualises the newest hourly sample and is 0 without samples", () => {
    expect(fundingNowPct([{ ts: 1, rateScaled: 100_000 }, { ts: 2, rateScaled: 174_600 }], annualisedPct)).toBeCloseTo(15.3, 1);
    expect(fundingNowPct([], annualisedPct)).toBe(0);
  });
});
