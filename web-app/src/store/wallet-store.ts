import { createStore } from "zustand/vanilla";
import { immer } from "zustand/middleware/immer";

export type WalletStatus = "disconnected" | "connecting" | "connected";

export interface WalletState {
  status: WalletStatus;
  /** Base58 address, empty string when not connected */
  address: string;
  /** True when the address is the mock demo wallet rather than an extension */
  demo: boolean;
}

export interface WalletActions {
  setWallet: (updates: Partial<WalletState>) => void;
  resetWallet: () => void;
}

export type WalletStore = WalletState & WalletActions;

const initial: WalletState = { status: "disconnected", address: "", demo: false };

export function createWalletStore() {
  return createStore<WalletStore>()(
    immer((set) => ({
      ...initial,
      setWallet(updates) {
        set((s) => {
          Object.assign(s, updates);
        });
      },
      resetWallet() {
        set((s) => {
          Object.assign(s, initial);
        });
      },
    })),
  );
}
