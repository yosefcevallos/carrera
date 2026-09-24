"use client";

import { useCallback, useEffect, useRef } from "react";
import { useSearchParams } from "next/navigation";
import { TICKERS, type Ticker } from "@/constants/vaults";
import { useUiStore } from "@/store/ui-provider";
import MyStrip from "./MyStrip";
import ProtocolStats from "./ProtocolStats";
import VaultModal from "./VaultModal";
import VaultTable from "./VaultTable";

export default function AppMain() {
  const openVault = useUiStore((s) => s.openVault);
  const open = useUiStore((s) => s.openVaultWindow);
  const params = useSearchParams();
  const lastFocus = useRef<HTMLElement | null>(null);

  // Deep link from the landing page: /app?v=TSLA opens that vault on Deposit.
  useEffect(() => {
    const v = params.get("v");
    if (v && (TICKERS as readonly string[]).includes(v)) open(v as Ticker, "deposit");
  }, [params, open]);

  useEffect(() => {
    if (openVault) lastFocus.current = document.activeElement as HTMLElement | null;
  }, [openVault]);

  const returnFocus = useCallback(() => lastFocus.current?.focus?.(), []);

  return (
    <main className="app">
      <div className="welcome2">
        <ProtocolStats />
        <MyStrip />
      </div>
      <VaultTable />
      {openVault && <VaultModal t={openVault} returnFocus={returnFocus} />}
    </main>
  );
}
