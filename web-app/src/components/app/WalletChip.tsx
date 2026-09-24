"use client";

import { useWallet } from "@solana/wallet-adapter-react";
import { useWalletModal } from "@solana/wallet-adapter-react-ui";
import { DATA_SOURCE } from "@/lib/chain/config";
import { DEMO_WALLET } from "@/lib/mock/world";
import { shortAddr } from "@/lib/format";
import { useUiStore } from "@/store/ui-provider";
import { useWalletStore } from "@/store/wallet-provider";

/** Connect / disconnect chip. Mock mode uses a demo wallet; rpc mode opens the wallet-adapter modal. */
export default function WalletChip() {
  const status = useWalletStore((s) => s.status);
  const address = useWalletStore((s) => s.address);
  const demo = useWalletStore((s) => s.demo);
  const setWallet = useWalletStore((s) => s.setWallet);
  const resetWallet = useWalletStore((s) => s.resetWallet);
  const showToast = useUiStore((s) => s.showToast);
  const { disconnect } = useWallet();
  const { setVisible } = useWalletModal();

  const on = status === "connected";

  function click() {
    if (on) {
      if (DATA_SOURCE === "mock") resetWallet();
      else disconnect().catch((err) => console.error("[WalletChip] disconnect failed:", err));
      showToast("Wallet disconnected.");
      return;
    }
    if (DATA_SOURCE === "mock") {
      setWallet({ status: "connected", address: DEMO_WALLET, demo: true });
      showToast("No wallet extension is used in demo mode, so you are using a demo wallet.");
      return;
    }
    setVisible(true);
  }

  return (
    <button className={`chip${on ? " on" : ""}`} onClick={click} aria-label={on ? `Wallet ${shortAddr(address)}, click to disconnect` : "Connect wallet"}>
      <span className="led" />
      <span>{on ? `${shortAddr(address)}${demo ? " (demo)" : ""}` : status === "connecting" ? "Connecting…" : "Connect wallet"}</span>
    </button>
  );
}
