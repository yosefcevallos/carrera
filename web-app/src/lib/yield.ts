import type { Mode, SharePricePoint, VaultRecord } from "./types";

export interface TrailingYield {
  /** Annualised realised yield on stock value, percent */
  apy: number;
  /** USDC per share gained over the window */
  usdcGained: number;
  /** Days actually covered by the window */
  days: number;
  /** True when the vault is younger than the requested window */
  sinceInception: boolean;
}

/**
 * Trailing realised growth of the share price in USDC terms, on stock value.
 * Never a projection: it only uses recorded history.
 */
export function trailingYield(history: SharePricePoint[], windowDays: number, priceUsd: number): TrailingYield {
  if (history.length < 2 || priceUsd <= 0) return { apy: 0, usdcGained: 0, days: 0, sinceInception: true };
  const last = history[history.length - 1];
  const span = history.length - 1;
  const days = Math.min(windowDays, span);
  const first = history[history.length - 1 - days];
  const usdcGained = last.usdcPerShare - first.usdcPerShare;
  const apy = days > 0 ? (usdcGained / priceUsd) * (365 / days) * 100 : 0;
  return { apy, usdcGained, days, sinceInception: span < windowDays };
}

/**
 * Realised yield for a display window. Prefers the indexer's growth figure for 7d and 30d when it
 * has one (rpc mode); otherwise derives it from the daily history (mock mode, or no indexer).
 * Growth bps of the share price in stock terms is a yield on stock value, annualised by 365/window.
 */
export function realisedYield(v: VaultRecord, windowDays: number): TrailingYield {
  const g = windowDays === 7 ? v.trailing.d7Bps : windowDays === 30 ? v.trailing.d30Bps : 0;
  if (g !== 0) {
    return { apy: (g / 100) * (365 / windowDays), usdcGained: (g / 10_000) * v.priceUsd, days: windowDays, sinceInception: false };
  }
  if (windowDays > 90 && v.trailing.inceptionBps !== 0 && v.trailing.inceptionDays > 0) {
    const d = v.trailing.inceptionDays;
    return { apy: (v.trailing.inceptionBps / 100) * (365 / d), usdcGained: (v.trailing.inceptionBps / 10_000) * v.priceUsd, days: d, sinceInception: true };
  }
  return trailingYield(v.sharePriceHistory, windowDays, v.priceUsd);
}

export interface RealisedGrowth {
  /** Raw growth of the share price in USDC terms over the window, percent of stock value. Never annualised. */
  growthPct: number;
  /** Days actually covered */
  days: number;
  /** True when the vault is younger than the requested window */
  sinceInception: boolean;
}

/**
 * Realised growth over a display window, as it happened: no extrapolation. Prefers the indexer's
 * growth bps for 7d/30d; otherwise the inception figure or the daily history. A young vault
 * reports what it has actually done since it opened.
 */
export function realisedGrowth(v: VaultRecord, windowDays: number): RealisedGrowth {
  const age = v.trailing.inceptionDays > 0 ? v.trailing.inceptionDays : v.ageDays;
  const g = windowDays === 7 ? v.trailing.d7Bps : windowDays === 30 ? v.trailing.d30Bps : 0;
  if (g !== 0 && age >= windowDays) return { growthPct: g / 100, days: windowDays, sinceInception: false };
  if (v.trailing.inceptionBps !== 0 || v.trailing.inceptionDays > 0) {
    return { growthPct: v.trailing.inceptionBps / 100, days: v.trailing.inceptionDays, sinceInception: age < windowDays };
  }
  const t = trailingYield(v.sharePriceHistory, windowDays, v.priceUsd);
  return { growthPct: v.priceUsd > 0 ? (t.usdcGained / v.priceUsd) * 100 : 0, days: t.days, sinceInception: t.sinceInception };
}

/** "+0.02% in 7d", or "−0.07% since inception, 1d" for a vault younger than the window. */
export function formatGrowth(g: RealisedGrowth, windowDays: number): string {
  const sign = g.growthPct < 0 ? "−" : "+";
  const pctText = `${sign}${Math.abs(g.growthPct).toFixed(2)}%`;
  if (g.sinceInception) return `${pctText} since inception${g.days > 0 ? `, ${g.days}d` : ""}`;
  return `${pctText} in ${windowDays}d`;
}

/** Which display window a vault is old enough for. */
export function availableWindows(ageDays: number): number[] {
  const w: number[] = [];
  if (ageDays >= 7) w.push(7);
  if (ageDays >= 30) w.push(30);
  return w;
}

export const modeLabel: Record<Mode, string> = {
  funding: "Funding",
  parked: "Parked",
  idle: "Idle",
};

export const modeLong: Record<Mode, string> = {
  funding: "Earning from funding",
  parked: "Earning from lending",
  idle: "Waiting for funding",
};

/**
 * Current annualised yield on stock value from the vault's on-chain rule inputs, bps (spec Part A):
 * Basis `L·f_avg − L(1+L)·r`, Parked `L·(s − r)`, Idle / Winding / Unwinding 0.
 */
export function currentApyBps(v: Pick<VaultRecord, "vaultState" | "ltvBps" | "fundingAvgBps" | "borrowApyBps" | "supplyApyBps">): number {
  const L = v.ltvBps / 10_000;
  if (v.vaultState === 3) return Math.round(L * v.fundingAvgBps - L * (1 + L) * v.borrowApyBps);
  if (v.vaultState === 1) return Math.round(L * (v.supplyApyBps - v.borrowApyBps));
  return 0;
}

/** Program funding unit: hourly rate in bps × 1e6. */
export const FUNDING_SCALE = 1_000_000;

/** Annualised percent for one hourly sample in the program's scaled unit. */
export function annualisedPct(rateScaled: number): number {
  return (rateScaled * 8760) / FUNDING_SCALE / 100;
}

/** Enter and exit bands around the hurdle, bps. */
export function bands(hurdleBps: number, enterMarginBps: number, exitMarginBps: number) {
  return { enterBps: hurdleBps + enterMarginBps, exitBps: hurdleBps - exitMarginBps };
}

/**
 * Redemption preview per spec §4.2: USDC out can be negative in a vault's early
 * life, in which case the stock leg is reduced instead.
 */
export function redemptionPreview(stockAmount: number, usdcEarned: number, priceUsd: number, exitFeeBps = 0) {
  let stockOut = stockAmount;
  let usdcOut = usdcEarned;
  let stockReduced = false;
  if (usdcOut < 0) {
    stockOut = priceUsd > 0 ? stockAmount - Math.abs(usdcOut) / priceUsd : stockAmount;
    usdcOut = 0;
    stockReduced = true;
  }
  const fee = usdcOut * (exitFeeBps / 10_000);
  return { stockOut: Math.max(0, stockOut), usdcOut: usdcOut - fee, stockReduced };
}
