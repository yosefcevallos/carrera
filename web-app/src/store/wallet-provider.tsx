"use client";

import { createContext, useContext, useState, type ReactNode } from "react";
import { useStore } from "zustand";
import { createWalletStore, type WalletStore } from "./wallet-store";

type WalletStoreApi = ReturnType<typeof createWalletStore>;
const WalletStoreContext = createContext<WalletStoreApi | null>(null);

export function WalletStoreProvider({ children }: { children: ReactNode }) {
  const [store] = useState(createWalletStore);
  return <WalletStoreContext.Provider value={store}>{children}</WalletStoreContext.Provider>;
}

export function useWalletStore<T>(selector: (state: WalletStore) => T): T {
  const store = useContext(WalletStoreContext);
  if (!store) throw new Error("useWalletStore must be used within WalletStoreProvider");
  return useStore(store, selector);
}
