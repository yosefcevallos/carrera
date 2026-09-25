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

/** One hourly Phoenix funding sample: unix ms and the program's scaled rate (bps × 1e6 per hour). */
export interface FundingSample {
  ts: number;
  rateScaled: number;
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
  /** Program VaultState: 0 Idle, 1 Parked, 2 Winding, 3 Basis, 4 Unwinding */
  vaultState: number;
  /** Tier borrow LTV, bps (params.ltv_bps) */
  ltvBps: number;
  /** Kamino USDC borrow APY the rule used, bps */
  borrowApyBps: number;
  /** Kamino USDC supply APY, bps */
  supplyApyBps: number;
  enterMarginBps: number;
  exitMarginBps: number;
  /** Hourly funding samples, up to 168 (7 days), oldest first */
  fundingSamples: FundingSample[];
  /** Days since the vault opened */
  ageDays: number;
  usdcPerShare: number;
  sharePriceHistory: SharePricePoint[];
  /** Indexed growth of the share price, bps, from the indexer's v_trailing_yield; 0 when unknown */
  trailing: TrailingGrowth;
}

export interface TrailingGrowth {
  d7Bps: number;
  d30Bps: number;
  inceptionBps: number;
  /** Days covered by inceptionBps; 0 when unknown */
  inceptionDays: number;
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

export type ExitStatus = "open" | "settled" | "redeemed" | "cancelled";

/** One exit request for a vault and wallet. */
export interface VaultExit {
  /** ExitRequest nonce, decimal string */
  nonce: string;
  shares: number;
  /** Stock paid or payable; equals shares until the epoch settles */
  stockAmount: number;
  /** USDC paid or payable; 0 until the epoch settles */
  usdcAmount: number;
  epochId: number;
  status: ExitStatus;
  /** Unix ms */
  requestedAt: number;
  /** Unix ms: top of the hour after the request, the earliest it can settle */
  readyAt: number;
}

export interface PositionsSnapshot {
  /** Display balances, raw / 10^decimals */
  balances: Record<Ticker, number>;
  positions: Record<Ticker, Position>;
  /** Exit requests per vault, newest first */
  exits: Record<Ticker, VaultExit[]>;
  /** Exact wallet balances in base units, as decimal strings (bigint is not structured-clone safe in every store path) */
  balancesRaw: Record<Ticker, string>;
  /** Exact share balances in base units */
  sharesRaw: Record<Ticker, string>;
  /** Decimals of the stock mint (shares use the same); 8 for xStocks */
  decimals: Record<Ticker, number>;
}
