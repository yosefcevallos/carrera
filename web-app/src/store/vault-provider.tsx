"use client";

import { createContext, useContext, useState, type ReactNode } from "react";
import { useStore } from "zustand";
import { createVaultStore, type VaultStore } from "./vault-store";

type VaultStoreApi = ReturnType<typeof createVaultStore>;
const VaultStoreContext = createContext<VaultStoreApi | null>(null);

export function VaultStoreProvider({ children }: { children: ReactNode }) {
  const [store] = useState(createVaultStore);
  return <VaultStoreContext.Provider value={store}>{children}</VaultStoreContext.Provider>;
}

export function useVaultStore<T>(selector: (state: VaultStore) => T): T {
  const store = useContext(VaultStoreContext);
  if (!store) throw new Error("useVaultStore must be used within VaultStoreProvider");
  return useStore(store, selector);
}
