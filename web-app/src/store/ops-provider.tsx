"use client";

import { createContext, useContext, useState, type ReactNode } from "react";
import { useStore } from "zustand";
import { createOpsStore, type OpsStore } from "./ops-store";

type OpsStoreApi = ReturnType<typeof createOpsStore>;
const OpsStoreContext = createContext<OpsStoreApi | null>(null);

export function OpsStoreProvider({ children }: { children: ReactNode }) {
  const [store] = useState(createOpsStore);
  return <OpsStoreContext.Provider value={store}>{children}</OpsStoreContext.Provider>;
}

export function useOpsStore<T>(selector: (state: OpsStore) => T): T {
  const store = useContext(OpsStoreContext);
  if (!store) throw new Error("useOpsStore must be used within OpsStoreProvider");
  return useStore(store, selector);
}
