"use client";

import { useEffect, useRef } from "react";
import { POSITION_POLL_MS } from "@/constants/vaults";
import { fetchPositions } from "@/lib/fetch-positions";
import { usePositionStore } from "@/store/position-provider";
import { useWalletStore } from "@/store/wallet-provider";

/** Wallet-bound hydrate phase: fetch on connect, poll while connected, reset on disconnect or address change. */
export default function PositionSyncer() {
  const status = useWalletStore((s) => s.status);
  const address = useWalletStore((s) => s.address);
  const setBalances = usePositionStore((s) => s.setBalances);
  const setPositions = usePositionStore((s) => s.setPositions);
  const setPendingExits = usePositionStore((s) => s.setPendingExits);
  const setRaw = usePositionStore((s) => s.setRaw);
  const resetPositions = usePositionStore((s) => s.resetPositions);
  const prevAddressRef = useRef<string>("");

  useEffect(() => {
    if (status !== "connected" || !address) {
      prevAddressRef.current = "";
      resetPositions();
      return;
    }
    if (prevAddressRef.current !== address) {
      prevAddressRef.current = address;
      resetPositions();
    }
    let alive = true;
    async function sync() {
      try {
        const snap = await fetchPositions(address);
        if (!alive) return;
        setBalances(snap.balances);
        setPositions(snap.positions);
        setPendingExits(snap.pendingExits);
        setRaw({ balancesRaw: snap.balancesRaw, sharesRaw: snap.sharesRaw, decimals: snap.decimals });
      } catch (err) {
        console.error("[PositionSyncer] fetch failed:", err);
      }
    }
    sync();
    const interval = setInterval(sync, POSITION_POLL_MS);
    return () => {
      alive = false;
      clearInterval(interval);
    };
  }, [status, address, setBalances, setPositions, setPendingExits, setRaw, resetPositions]);

  return null;
}
