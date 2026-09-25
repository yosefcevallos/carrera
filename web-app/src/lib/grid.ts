import { TICKERS, type Ticker } from "@/constants/vaults";
import type { VaultRecord } from "./types";
import { currentApyBps } from "./yield";

export interface GridEntry {
  ticker: Ticker;
  /** Grid position, 1-based */
  pos: number;
  apyBps: number;
  /** "funding" | "parked" | "idle" for the strip label */
  mode: "funding" | "parked" | "idle";
}

const modeOf = (v: VaultRecord): GridEntry["mode"] => (v.vaultState === 3 ? "funding" : v.vaultState === 1 ? "parked" : "idle");

/** Every vault ranked by current APY, highest first; ties broken by TVL, then ticker order. */
export function rankVaults(vaults: Record<Ticker, VaultRecord>): GridEntry[] {
  return [...TICKERS]
    .map((t) => ({ t, apy: currentApyBps(vaults[t]), tvl: vaults[t].tvlUsd }))
    .sort((a, b) => b.apy - a.apy || b.tvl - a.tvl || TICKERS.indexOf(a.t) - TICKERS.indexOf(b.t))
    .map((x, i) => ({ ticker: x.t, pos: i + 1, apyBps: x.apy, mode: modeOf(vaults[x.t]) }));
}

export interface PoleSummary {
  leader: GridEntry;
  p2?: GridEntry;
  p3?: GridEntry;
  /** Leader minus P2, in percentage points */
  gapPts: number;
}

/** Pole position readout, or null when no vault is earning (every APY is 0). */
export function poleSummary(vaults: Record<Ticker, VaultRecord>): PoleSummary | null {
  const grid = rankVaults(vaults);
  if (grid.length === 0 || grid[0].apyBps <= 0) return null;
  const [leader, p2, p3] = grid;
  return { leader, p2, p3, gapPts: (leader.apyBps - (p2?.apyBps ?? 0)) / 100 };
}
