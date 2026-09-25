"use client";

import { useEffect, useState } from "react";
import { DATA_SOURCE } from "@/lib/chain/config";
import { agoLabel, formatCountdown, hourUtc, lastFundingTs, nextTopOfHour } from "@/lib/app2";
import { useVaultStore } from "@/store/vault-provider";

const SLOT_POLL_MS = 15_000;
/** Demo slot when there is no chain to ask: advances at Solana's rough 2.5 slots/s. */
const MOCK_SLOT_BASE = 312_884_102;

/**
 * Signs of a live system: the funding cycle the vaults last recorded, a countdown to the next top
 * of the hour, the current Solana slot and the age of the last successful vault poll.
 */
export default function StatusLine() {
  const vaults = useVaultStore((s) => s.vaults);
  const syncedAt = useVaultStore((s) => s.syncedAt);
  const [now, setNow] = useState(() => Date.now());
  const [slot, setSlot] = useState(0);
  const [mounted, setMounted] = useState(false);

  useEffect(() => {
    setMounted(true);
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);

  useEffect(() => {
    let alive = true;
    const started = Date.now();
    async function poll() {
      if (DATA_SOURCE === "mock") {
        setSlot(MOCK_SLOT_BASE + Math.floor((Date.now() - started) / 400));
        return;
      }
      try {
        const { connection } = await import("@/lib/chain/rpc");
        const s = await connection().getSlot("confirmed");
        if (alive) setSlot(s);
      } catch (err) {
        console.error("[StatusLine] getSlot failed:", err);
      }
    }
    poll();
    const t = setInterval(poll, SLOT_POLL_MS);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);

  const cycle = lastFundingTs(vaults);
  return (
    <div className="status mono" aria-live="off">
      <span className="live">
        <i />
        LIVE
      </span>
      <span className="cycle">
        Funding cycle <b>{cycle ? hourUtc(cycle) : "—"}</b>
      </span>
      <span>
        Next in <b>{mounted ? formatCountdown(nextTopOfHour(now) - now) : "--:--"}</b>
      </span>
      <span className="slot">
        Slot <b>{slot ? slot.toLocaleString("en-US") : "—"}</b>
      </span>
      <span className="sp">
        Updated <b>{mounted ? agoLabel(syncedAt, now) : "—"}</b>
      </span>
    </div>
  );
}
