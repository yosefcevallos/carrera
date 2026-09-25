"use client";

import { useCallback } from "react";
import { fetchPositions } from "./fetch-positions";
import { fetchVaults } from "./fetch-vaults";
import { usePositionStore } from "@/store/position-provider";
import { useVaultStore } from "@/store/vault-provider";
import { useWalletStore } from "@/store/wallet-provider";

/**
 * Refresh phase: after a deposit, exit or redeem, re-run the same fetchers and
 * setters the syncers use. Returns a function that never throws; errors are logged.
 */
export function useRefresh() {
  const address = useWalletStore((s) => s.address);
  const setProtocol = useVaultStore((s) => s.setProtocol);
  const setVaults = useVaultStore((s) => s.setVaults);
  const setBalances = usePositionStore((s) => s.setBalances);
  const setPositions = usePositionStore((s) => s.setPositions);
  const setPendingExits = usePositionStore((s) => s.setPendingExits);
  const setRaw = usePositionStore((s) => s.setRaw);

  return useCallback(() => {
    fetchVaults()
      .then((snap) => {
        setProtocol(snap.protocol);
        setVaults(snap.vaults);
      })
      .catch((err) => console.error("[refresh] vaults failed:", err));
    if (address) {
      fetchPositions(address)
        .then((snap) => {
          setBalances(snap.balances);
          setPositions(snap.positions);
          setPendingExits(snap.pendingExits);
        setRaw({ balancesRaw: snap.balancesRaw, sharesRaw: snap.sharesRaw, decimals: snap.decimals });
        })
        .catch((err) => console.error("[refresh] positions failed:", err));
    }
  }, [address, setProtocol, setVaults, setBalances, setPositions, setPendingExits, setRaw]);
}
