"use client";

import { createContext, useContext, useState, type ReactNode } from "react";
import { useStore } from "zustand";
import { createUiStore, type UiStore } from "./ui-store";

type UiStoreApi = ReturnType<typeof createUiStore>;
const UiStoreContext = createContext<UiStoreApi | null>(null);

export function UiStoreProvider({ children }: { children: ReactNode }) {
  const [store] = useState(createUiStore);
  return <UiStoreContext.Provider value={store}>{children}</UiStoreContext.Provider>;
}

export function useUiStore<T>(selector: (state: UiStore) => T): T {
  const store = useContext(UiStoreContext);
  if (!store) throw new Error("useUiStore must be used within UiStoreProvider");
  return useStore(store, selector);
}
