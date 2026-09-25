import { createStore } from "zustand/vanilla";
import { immer } from "zustand/middleware/immer";
import type { Ticker } from "@/constants/vaults";

export type Filter = "all" | "funding" | "positions";
export type Tab = "deposit" | "withdraw" | "requests";
export type ChartRange = 7 | 30 | 90;

export interface UiState {
  /** Landing: the highlighted bubble */
  selected: Ticker;
  filter: Filter;
  /** Empty string when no vault window is open */
  openVault: Ticker | "";
  tab: Tab;
  range: ChartRange;
  detailsOpen: boolean;
  toast: string;
  toastSeq: number;
}

export interface UiActions {
  select: (t: Ticker) => void;
  setFilter: (f: Filter) => void;
  openVaultWindow: (t: Ticker, tab: Tab) => void;
  closeVaultWindow: () => void;
  setTab: (tab: Tab) => void;
  setRange: (r: ChartRange) => void;
  toggleDetails: () => void;
  showToast: (message: string) => void;
  resetUi: () => void;
}

export type UiStore = UiState & UiActions;

const initial: UiState = {
  selected: "TSLA",
  filter: "all",
  openVault: "",
  tab: "deposit",
  range: 30,
  detailsOpen: false,
  toast: "",
  toastSeq: 0,
};

export function createUiStore() {
  return createStore<UiStore>()(
    immer((set) => ({
      ...initial,
      select(t) {
        set((s) => {
          s.selected = t;
        });
      },
      setFilter(f) {
        set((s) => {
          s.filter = f;
        });
      },
      openVaultWindow(t, tab) {
        set((s) => {
          s.openVault = t;
          s.tab = tab;
          s.range = 30;
          s.detailsOpen = false;
        });
      },
      closeVaultWindow() {
        set((s) => {
          s.openVault = "";
        });
      },
      setTab(tab) {
        set((s) => {
          s.tab = tab;
        });
      },
      setRange(r) {
        set((s) => {
          s.range = r;
        });
      },
      toggleDetails() {
        set((s) => {
          s.detailsOpen = !s.detailsOpen;
        });
      },
      showToast(message) {
        set((s) => {
          s.toast = message;
          s.toastSeq += 1;
        });
      },
      resetUi() {
        set((s) => {
          Object.assign(s, initial);
        });
      },
    })),
  );
}
