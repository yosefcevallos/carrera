import { createStore } from "zustand/vanilla";
import { immer } from "zustand/middleware/immer";
import { TICKERS, type Ticker } from "@/constants/vaults";
import { type Health, type HistoryPoint, type KeeperView, type VaultBook, zeroBook, zeroKeeper } from "@/lib/ops-types";
import { filled } from "@/lib/zeroed";

export interface OpsState {
  keeper: KeeperView;
  books: Record<Ticker, VaultBook>;
  history: Record<Ticker, HistoryPoint[]>;
  selected: Ticker;
  health: Health;
  /** Unix ms of the last successful status sync; 0 before the first */
  syncedAt: number;
}

export interface OpsActions {
  /** Replace: the keeper block is authoritative. */
  setKeeper: (keeper: KeeperView) => void;
  /** Merge: keys absent from the update keep their current book. */
  setBooks: (updates: Partial<Record<Ticker, VaultBook>>) => void;
  /** Replace one vault's history. */
  setHistory: (ticker: Ticker, points: HistoryPoint[]) => void;
  select: (ticker: Ticker) => void;
  setHealth: (health: Health) => void;
  setSyncedAt: (ts: number) => void;
  resetOps: () => void;
}

export type OpsStore = OpsState & OpsActions;

const initial = (): OpsState => ({
  keeper: zeroKeeper(),
  books: filled(TICKERS, () => zeroBook()),
  history: filled(TICKERS, () => [] as HistoryPoint[]),
  selected: "TSLA",
  health: "unknown",
  syncedAt: 0,
});

export function createOpsStore() {
  return createStore<OpsStore>()(
    immer((set) => ({
      ...initial(),
      setKeeper(keeper) {
        set((s) => {
          s.keeper = keeper;
        });
      },
      setBooks(updates) {
        set((s) => {
          for (const [ticker, book] of Object.entries(updates)) {
            if (book) s.books[ticker as Ticker] = book;
          }
        });
      },
      setHistory(ticker, points) {
        set((s) => {
          s.history[ticker] = points;
        });
      },
      select(ticker) {
        set((s) => {
          s.selected = ticker;
        });
      },
      setHealth(health) {
        set((s) => {
          s.health = health;
        });
      },
      setSyncedAt(ts) {
        set((s) => {
          s.syncedAt = ts;
        });
      },
      resetOps() {
        set((s) => {
          const fresh = initial();
          s.keeper = fresh.keeper;
          s.books = fresh.books;
          s.history = fresh.history;
          s.health = fresh.health;
          s.syncedAt = 0;
        });
      },
    })),
  );
}
