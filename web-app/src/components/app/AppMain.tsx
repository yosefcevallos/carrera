"use client";

import { useCallback, useEffect, useRef } from "react";
import { useSearchParams } from "next/navigation";
import { useWalletModal } from "@solana/wallet-adapter-react-ui";
import { TICKERS, type Ticker } from "@/constants/vaults";
import { DATA_SOURCE } from "@/lib/chain/config";
import { DEMO_WALLET } from "@/lib/mock/world";
import { useUiStore } from "@/store/ui-provider";
import { useWalletStore } from "@/store/wallet-provider";
import StatusLine from "./StatusLine";
import SummaryBar from "./SummaryBar";
import VaultModal from "./VaultModal";
import VaultTable from "./VaultTable";

export default function AppMain() {
  const openVault = useUiStore((s) => s.openVault);
  const open = useUiStore((s) => s.openVaultWindow);
  const setWallet = useWalletStore((s) => s.setWallet);
  const showToast = useUiStore((s) => s.showToast);
  const { setVisible } = useWalletModal();
  const params = useSearchParams();
  const lastFocus = useRef<HTMLElement | null>(null);

  // Deep link: /app?v=TSLA opens that vault on Deposit.
  useEffect(() => {
    const v = params.get("v");
    if (v && (TICKERS as readonly string[]).includes(v)) open(v as Ticker, "deposit");
  }, [params, open]);

  useEffect(() => {
    if (openVault) lastFocus.current = document.activeElement as HTMLElement | null;
  }, [openVault]);

  const returnFocus = useCallback(() => lastFocus.current?.focus?.(), []);
  const onConnect = useCallback(() => {
    if (DATA_SOURCE === "mock") {
      setWallet({ status: "connected", address: DEMO_WALLET, demo: true });
      showToast("Demo wallet connected.");
    } else setVisible(true);
  }, [setWallet, showToast, setVisible]);

  return (
    <>
      <StatusLine />
      <main className="wrap2">
        <SummaryBar onConnect={onConnect} />
        <VaultTable />
        {openVault && <VaultModal t={openVault} returnFocus={returnFocus} />}
      </main>
    </>
  );
}
