"use client";

import { createContext, useContext, useState, type ReactNode } from "react";
import { useStore } from "zustand";
import { createPositionStore, type PositionStore } from "./position-store";

type PositionStoreApi = ReturnType<typeof createPositionStore>;
const PositionStoreContext = createContext<PositionStoreApi | null>(null);

export function PositionStoreProvider({ children }: { children: ReactNode }) {
  const [store] = useState(createPositionStore);
  return <PositionStoreContext.Provider value={store}>{children}</PositionStoreContext.Provider>;
}

export function usePositionStore<T>(selector: (state: PositionStore) => T): T {
  const store = useContext(PositionStoreContext);
  if (!store) throw new Error("usePositionStore must be used within PositionStoreProvider");
  return useStore(store, selector);
}
