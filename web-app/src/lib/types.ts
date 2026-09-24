import type { Ticker } from "@/constants/vaults";

/** Where the borrowed USDC is. Matches the program's VaultState collapsed for display. */
export type Mode = "funding" | "parked" | "idle";

export interface SharePricePoint {
  /** ISO date, one per day */
  date: string;
  /** USDC accrued per share; can be slightly negative early in a vault's life */
  usdcPerShare: number;
  mode: Mode;
}

export interface VaultRecord {
  mode: Mode;
  marketOpen: boolean;
  /** Oracle price of the xStock in USD */
  priceUsd: number;
  tvlUsd: number;
  capUsd: number;
  totalShares: number;
  /** 24h rolling funding average, annualised, bps */
  fundingAvgBps: number;
  /** Hurdle the rule compared against, bps (from RuleEvaluated) */
  hurdleBps: number;
  enterMarginBps: number;
  exitMarginBps: number;
  /** Hourly funding rates for the last 24 hours, annualised percent, oldest first */
  funding24h: number[];
  /** Days since the vault opened */
  ageDays: number;
  usdcPerShare: number;
  sharePriceHistory: SharePricePoint[];
}

export interface ProtocolStats {
  tvlUsd: number;
  /** TVL-weighted trailing 30d realised yield, bps */
  avgYieldBps: number;
  vaultsInFunding: number;
  usdcPaid24h: number;
  depositors: number;
  /** Kamino USDC reserve rates, bps */
  borrowApyBps: number;
  supplyApyBps: number;
}

export interface VaultsSnapshot {
  protocol: ProtocolStats;
  vaults: Record<Ticker, VaultRecord>;
}

export interface Position {
  shares: number;
  stockAmount: number;
  /** USDC attributable to these shares at current NAV, can be negative early */
  usdcEarned: number;
}

export interface PendingExit {
  shares: number;
  stockAmount: number;
  usdcAmount: number;
  /** Unix ms when the epoch settles; 0 when no exit is pending */
  readyAt: number;
  ready: boolean;
  nonce: number;
}

export interface PositionsSnapshot {
  balances: Record<Ticker, number>;
  positions: Record<Ticker, Position>;
  pendingExits: Record<Ticker, PendingExit>;
}
