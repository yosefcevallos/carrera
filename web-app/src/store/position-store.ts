import { createStore } from "zustand/vanilla";
import { immer } from "zustand/middleware/immer";
import { TICKERS, type Ticker } from "@/constants/vaults";
import type { Position, VaultExit } from "@/lib/types";
import { filled, zeroed } from "@/lib/zeroed";

export interface PositionState {
  /** xStock balances in the wallet */
  balances: Record<Ticker, number>;
  positions: Record<Ticker, Position>;
  /** Exit requests per vault, newest first; wholesale-replaced by each fetch */
  exits: Record<Ticker, VaultExit[]>;
  /** Exact base-unit amounts as decimal strings; "0" before the first fetch */
  balancesRaw: Record<Ticker, string>;
  sharesRaw: Record<Ticker, string>;
  decimals: Record<Ticker, number>;
}

export interface PositionActions {
  setBalances: (updates: Partial<Record<Ticker, number>>) => void;
  setPositions: (updates: Partial<Record<Ticker, Position>>) => void;
  setExits: (updates: Partial<Record<Ticker, VaultExit[]>>) => void;
  /** Prepend a request this app just sent, so it shows before the next fetch */
  addExit: (t: Ticker, exit: VaultExit) => void;
  /** Optimistic status change for one request, e.g. redeemed the moment the claim confirms */
  setExitStatus: (t: Ticker, nonce: string, status: VaultExit["status"]) => void;
  setRaw: (updates: { balancesRaw?: Partial<Record<Ticker, string>>; sharesRaw?: Partial<Record<Ticker, string>>; decimals?: Partial<Record<Ticker, number>> }) => void;
  resetPositions: () => void;
}

export type PositionStore = PositionState & PositionActions;

export const zeroPosition = (): Position => ({ shares: 0, stockAmount: 0, usdcEarned: 0 });
export const zeroRaw = () => filled(TICKERS, () => "0");
export const defaultDecimals = () => filled(TICKERS, () => 8);
export const zeroExits = (): VaultExit[] => [];

export function createPositionStore() {
  return createStore<PositionStore>()(
    immer((set) => ({
      balances: zeroed(TICKERS),
      positions: filled(TICKERS, zeroPosition),
      exits: filled(TICKERS, zeroExits),
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
      setExits(updates) {
        set((s) => {
          for (const [t, v] of Object.entries(updates)) if (v) s.exits[t as Ticker] = v;
        });
      },
      addExit(t, exit) {
        set((s) => {
          s.exits[t] = [exit, ...s.exits[t].filter((e) => e.nonce !== exit.nonce)];
        });
      },
      setExitStatus(t, nonce, status) {
        set((s) => {
          const e = s.exits[t].find((x) => x.nonce === nonce);
          if (e) e.status = status;
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
          s.exits = filled(TICKERS, zeroExits);
          s.balancesRaw = zeroRaw();
          s.sharesRaw = zeroRaw();
          s.decimals = defaultDecimals();
        });
      },
    })),
  );
}
