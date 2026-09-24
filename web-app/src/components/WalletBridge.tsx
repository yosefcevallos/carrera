"use client";

import { useEffect } from "react";
import { useWallet } from "@solana/wallet-adapter-react";
import { useWalletStore } from "@/store/wallet-provider";
import { DATA_SOURCE } from "@/lib/chain/config";

/** Mirrors wallet-adapter state into the wallet store. In mock mode the chip manages a demo wallet instead. */
export default function WalletBridge() {
  const { publicKey, connected, connecting } = useWallet();
  const setWallet = useWalletStore((s) => s.setWallet);

  useEffect(() => {
    if (DATA_SOURCE === "mock") return;
    setWallet({
      status: connected ? "connected" : connecting ? "connecting" : "disconnected",
      address: connected && publicKey ? publicKey.toBase58() : "",
      demo: false,
    });
  }, [publicKey, connected, connecting, setWallet]);

  return null;
}
