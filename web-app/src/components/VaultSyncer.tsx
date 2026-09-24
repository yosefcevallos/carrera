"use client";

import { useEffect } from "react";
import { VAULT_POLL_MS } from "@/constants/vaults";
import { fetchVaults } from "@/lib/fetch-vaults";
import { useVaultStore } from "@/store/vault-provider";

/** Global hydrate phase: fetch once on mount, poll while mounted. Not tied to a wallet. */
export default function VaultSyncer() {
  const setProtocol = useVaultStore((s) => s.setProtocol);
  const setVaults = useVaultStore((s) => s.setVaults);
  const setSyncedAt = useVaultStore((s) => s.setSyncedAt);

  useEffect(() => {
    let alive = true;
    async function sync() {
      try {
        const snap = await fetchVaults(); // network work outside set()
        if (!alive) return;
        setProtocol(snap.protocol); // commit through setters
        setVaults(snap.vaults);
        setSyncedAt(Date.now());
      } catch (err) {
        console.error("[VaultSyncer] fetch failed:", err);
      }
    }
    sync();
    const interval = setInterval(sync, VAULT_POLL_MS);
    return () => {
      alive = false;
      clearInterval(interval);
    };
  }, [setProtocol, setVaults, setSyncedAt]);

  return null;
}
