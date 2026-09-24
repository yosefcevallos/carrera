import { createStore } from "zustand/vanilla";
import { immer } from "zustand/middleware/immer";
import { TICKERS, type Ticker } from "@/constants/vaults";
import type { ProtocolStats, VaultRecord } from "@/lib/types";
import { filled } from "@/lib/zeroed";

export interface VaultState {
  protocol: ProtocolStats;
  vaults: Record<Ticker, VaultRecord>;
  /** Unix ms of the last successful sync; 0 before the first */
  syncedAt: number;
}

export interface VaultActions {
  setProtocol: (updates: Partial<ProtocolStats>) => void;
  /** Merge: keys absent from the update keep their current record. */
  setVaults: (updates: Partial<Record<Ticker, VaultRecord>>) => void;
  setSyncedAt: (ts: number) => void;
  resetVaults: () => void;
}

export type VaultStore = VaultState & VaultActions;

export function zeroVault(): VaultRecord {
  return {
    mode: "idle",
    marketOpen: false,
    priceUsd: 0,
    tvlUsd: 0,
    capUsd: 0,
    totalShares: 0,
    fundingAvgBps: 0,
    hurdleBps: 0,
    enterMarginBps: 0,
    exitMarginBps: 0,
    funding24h: [],
    ageDays: 0,
    usdcPerShare: 0,
    sharePriceHistory: [],
  };
}

const initialProtocol: ProtocolStats = {
  tvlUsd: 0,
  avgYieldBps: 0,
  vaultsInFunding: 0,
  usdcPaid24h: 0,
  depositors: 0,
  borrowApyBps: 0,
  supplyApyBps: 0,
};

export function createVaultStore() {
  return createStore<VaultStore>()(
    immer((set) => ({
      protocol: { ...initialProtocol },
      vaults: filled(TICKERS, zeroVault),
      syncedAt: 0,

      setProtocol(updates) {
        set((s) => {
          Object.assign(s.protocol, updates);
        });
      },
      setVaults(updates) {
        set((s) => {
          for (const [ticker, rec] of Object.entries(updates)) {
            if (rec) s.vaults[ticker as Ticker] = rec;
          }
        });
      },
      setSyncedAt(ts) {
        set((s) => {
          s.syncedAt = ts;
        });
      },
      resetVaults() {
        set((s) => {
          s.protocol = { ...initialProtocol };
          s.vaults = filled(TICKERS, zeroVault);
          s.syncedAt = 0;
        });
      },
    })),
  );
}
