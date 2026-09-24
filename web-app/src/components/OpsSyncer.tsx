"use client";

import { useEffect } from "react";
import { OPS_HISTORY_POLL_MS, OPS_STATUS_POLL_MS } from "@/constants/vaults";
import { fetchOps, fetchOpsHealth, fetchOpsHistory } from "@/lib/fetch-ops";
import { useOpsStore } from "@/store/ops-provider";

/** Mounted on the /ops route only: polls the keeper's status server while the page is open. */
export default function OpsSyncer() {
  const selected = useOpsStore((s) => s.selected);
  const setKeeper = useOpsStore((s) => s.setKeeper);
  const setBooks = useOpsStore((s) => s.setBooks);
  const setHistory = useOpsStore((s) => s.setHistory);
  const setHealth = useOpsStore((s) => s.setHealth);
  const setSyncedAt = useOpsStore((s) => s.setSyncedAt);

  useEffect(() => {
    let alive = true;
    async function sync() {
      try {
        const [snap, health] = await Promise.all([fetchOps(), fetchOpsHealth()]); // network work outside set()
        if (!alive) return;
        setKeeper(snap.keeper); // commit through setters
        setBooks(snap.books);
        setHealth(health);
        setSyncedAt(Date.now());
      } catch (err) {
        console.error("[OpsSyncer] status fetch failed:", err);
        if (alive) setHealth("stale");
      }
    }
    sync();
    const interval = setInterval(sync, OPS_STATUS_POLL_MS);
    return () => {
      alive = false;
      clearInterval(interval);
    };
  }, [setKeeper, setBooks, setHealth, setSyncedAt]);

  useEffect(() => {
    let alive = true;
    async function sync() {
      try {
        const points = await fetchOpsHistory(selected, 168);
        if (alive) setHistory(selected, points);
      } catch (err) {
        console.error("[OpsSyncer] history fetch failed:", err);
      }
    }
    sync();
    const interval = setInterval(sync, OPS_HISTORY_POLL_MS);
    return () => {
      alive = false;
      clearInterval(interval);
    };
  }, [selected, setHistory]);

  return null;
}
