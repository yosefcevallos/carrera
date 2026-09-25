// Static configuration for the nine vaults. Skeleton stores are built from these keys.

export const TICKERS = [
  "MSTR",
  "SPY",
  "GOOGL",
  "QQQ",
  "TSLA",
  "NVDA",
  "CRCL",
  "HOOD",
  "AAPL",
] as const;

export type Ticker = (typeof TICKERS)[number];

export type Tier = "A" | "B" | "C" | "D";

export interface VaultMeta {
  roundel: number;
  name: string;
  token: string;
  tier: Tier;
  ltvBps: number;
  /** Stock move before liquidation, percent. Down is Kamino, up is Phoenix. */
  liqBuffer: { down: number; up: number };
}

export const VAULT_META: Record<Ticker, VaultMeta> = {
  MSTR: { roundel: 1, name: "Strategy", token: "MSTRx", tier: "D", ltvBps: 2000, liqBuffer: { down: -50, up: 16 } },
  SPY: { roundel: 2, name: "SPDR S&P 500", token: "SPYx", tier: "A", ltvBps: 3000, liqBuffer: { down: -57, up: 26 } },
  GOOGL: { roundel: 3, name: "Alphabet", token: "GOOGLx", tier: "A", ltvBps: 3000, liqBuffer: { down: -57, up: 26 } },
  QQQ: { roundel: 4, name: "Invesco QQQ", token: "QQQx", tier: "A", ltvBps: 3000, liqBuffer: { down: -57, up: 26 } },
  TSLA: { roundel: 5, name: "Tesla", token: "TSLAx", tier: "B", ltvBps: 3000, liqBuffer: { down: -54, up: 26 } },
  NVDA: { roundel: 6, name: "NVIDIA", token: "NVDAx", tier: "B", ltvBps: 3000, liqBuffer: { down: -54, up: 26 } },
  CRCL: { roundel: 7, name: "Circle", token: "CRCLx", tier: "D", ltvBps: 2000, liqBuffer: { down: -50, up: 16 } },
  HOOD: { roundel: 8, name: "Robinhood", token: "HOODx", tier: "D", ltvBps: 2000, liqBuffer: { down: -50, up: 16 } },
  AAPL: { roundel: 9, name: "Apple", token: "AAPLx", tier: "C", ltvBps: 2500, liqBuffer: { down: -50, up: 21 } },
};


export const VAULT_POLL_MS = 60_000;
export const POSITION_POLL_MS = 30_000;
export const PERF_FEE_BPS = 1500;
export const EXIT_FEE_BPS = 10;
export const OPS_STATUS_POLL_MS = 10_000;
export const OPS_HISTORY_POLL_MS = 60_000;
