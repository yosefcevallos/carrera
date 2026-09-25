import { createStore } from "zustand/vanilla";
import { immer } from "zustand/middleware/immer";
import { TICKERS, type Ticker } from "@/constants/vaults";
import type { PendingExit, Position } from "@/lib/types";
import { filled, zeroed } from "@/lib/zeroed";

export interface PositionState {
  /** xStock balances in the wallet */
  balances: Record<Ticker, number>;
  positions: Record<Ticker, Position>;
  pendingExits: Record<Ticker, PendingExit>;
  /** Exact base-unit amounts as decimal strings; "0" before the first fetch */
  balancesRaw: Record<Ticker, string>;
  sharesRaw: Record<Ticker, string>;
  decimals: Record<Ticker, number>;
}

export interface PositionActions {
  setBalances: (updates: Partial<Record<Ticker, number>>) => void;
  setPositions: (updates: Partial<Record<Ticker, Position>>) => void;
  setPendingExits: (updates: Partial<Record<Ticker, PendingExit>>) => void;
  setRaw: (updates: { balancesRaw?: Partial<Record<Ticker, string>>; sharesRaw?: Partial<Record<Ticker, string>>; decimals?: Partial<Record<Ticker, number>> }) => void;
  resetPositions: () => void;
}

export type PositionStore = PositionState & PositionActions;

export const zeroPosition = (): Position => ({ shares: 0, stockAmount: 0, usdcEarned: 0 });
export const zeroRaw = () => filled(TICKERS, () => "0");
export const defaultDecimals = () => filled(TICKERS, () => 8);
export const zeroExit = (): PendingExit => ({ shares: 0, stockAmount: 0, usdcAmount: 0, readyAt: 0, ready: false, nonce: 0 });

export function createPositionStore() {
  return createStore<PositionStore>()(
    immer((set) => ({
      balances: zeroed(TICKERS),
      positions: filled(TICKERS, zeroPosition),
      pendingExits: filled(TICKERS, zeroExit),
      balancesRaw: zeroRaw(),
      sharesRaw: zeroRaw(),
      decimals: defaultDecimals(),

      setBalances(updates) {
        set((s) => {
          for (const [t, v] of Object.entries(updates)) if (v !== undefined) s.balances[t as Ticker] = v;
        });
      },
      setPositions(updates) {
        set((s) => {
          for (const [t, v] of Object.entries(updates)) if (v) s.positions[t as Ticker] = v;
        });
      },
      setPendingExits(updates) {
        set((s) => {
          for (const [t, v] of Object.entries(updates)) if (v) s.pendingExits[t as Ticker] = v;
        });
      },
      setRaw(updates) {
        set((s) => {
          for (const [t, v] of Object.entries(updates.balancesRaw ?? {})) if (v !== undefined) s.balancesRaw[t as Ticker] = v;
          for (const [t, v] of Object.entries(updates.sharesRaw ?? {})) if (v !== undefined) s.sharesRaw[t as Ticker] = v;
          for (const [t, v] of Object.entries(updates.decimals ?? {})) if (v !== undefined) s.decimals[t as Ticker] = v;
        });
      },
      resetPositions() {
        set((s) => {
          s.balances = zeroed(TICKERS);
          s.positions = filled(TICKERS, zeroPosition);
          s.pendingExits = filled(TICKERS, zeroExit);
          s.balancesRaw = zeroRaw();
          s.sharesRaw = zeroRaw();
          s.decimals = defaultDecimals();
        });
      },
    })),
  );
}
