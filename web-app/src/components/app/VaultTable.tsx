"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { TICKERS, VAULT_META, type Ticker } from "@/constants/vaults";
import TokenIcon from "@/components/TokenIcon";
import { fmt, usd } from "@/lib/format";
import { anyOpen, anyReady } from "@/lib/exits";
import { filterRows, sharedScale, sortRows, sparkRaw, sparkSeries, type RowInput, type SortDir, type SortKey } from "@/lib/app2";
import { currentApyBps, modeLong } from "@/lib/yield";
import { usePositionStore } from "@/store/position-provider";
import { useUiStore } from "@/store/ui-provider";
import { useVaultStore } from "@/store/vault-provider";
import { useWalletStore } from "@/store/wallet-provider";
import type { Filter } from "@/store/ui-store";
import Sparkline from "./Sparkline";

const SEGMENTS: { key: Filter; label: string }[] = [
  { key: "all", label: "All" },
  { key: "funding", label: "Funding" },
  { key: "positions", label: "Positions" },
];

const FLASH_MS = 1200;

/** APY figure that flashes for a moment when a poll changes it. */
function Apy({ bps, idle, avgBps }: { bps: number; idle: boolean; avgBps: number }) {
  const prev = useRef(bps);
  const [flash, setFlash] = useState(false);
  useEffect(() => {
    if (prev.current === bps) return;
    prev.current = bps;
    setFlash(true);
    const t = setTimeout(() => setFlash(false), FLASH_MS);
    return () => clearTimeout(t);
  }, [bps]);
  return (
    <span className="apy-cell">
      <span className={`apy mono${idle ? " idle" : ""}`}>
        <i />
        {idle ? "Idle" : <span className={flash ? "flash" : ""}>{fmt(bps / 100, 2)}%</span>}
      </span>
      <span className="apy-sub mono">24h avg funding {fmt(avgBps / 100, 1)}%</span>
    </span>
  );
}

function Row({ r, max }: { r: RowInput; max: number }) {
  const { t, v, p, exits } = r;
  const open = useUiStore((s) => s.openVaultWindow);
  const meta = VAULT_META[t];
  const yours = p.shares > 0;
  const ready = anyReady(exits);
  const settling = anyOpen(exits);
  const idle = v.vaultState !== 3 && v.vaultState !== 1;
  const funding = v.mode === "funding";
  const value = p.stockAmount * v.priceUsd;
  const openIt = () => open(t, ready || settling ? "requests" : yours ? "withdraw" : "deposit");

  return (
    <tr
      className="row"
      tabIndex={0}
      aria-label={`${t}, ${meta.name}. ${modeLong[v.mode]}. Open vault`}
      onClick={openIt}
      onKeyDown={(ev) => {
        if (ev.key === "Enter" || ev.key === " ") {
          ev.preventDefault();
          openIt();
        }
      }}
    >
      <td>
        <span className="as">
          <TokenIcon t={t} size={24} eager />
          <span className="as-t">
            <b>{t}</b>
            <span className="co">{meta.name}</span>
            <span className="mob mono">{yours ? usd(value, 2) : meta.name}</span>
          </span>
        </span>
      </td>
      <td className="r">
        <Apy bps={currentApyBps(v)} idle={idle} avgBps={v.fundingAvgBps} />
      </td>
      <td className="r c-spark">
        <Sparkline series={sparkSeries(v.fundingSamples)} raw={sparkRaw(v.fundingSamples)} max={max} funding={funding} />
      </td>
      <td className="r pos c-pos">
        {yours || settling || ready ? (
          <>
            <b className="mono">{usd(value, 2)}</b>
            <span className={`mono${ready ? " ready" : ""}`}>
              {ready ? "Ready to claim" : settling ? `${exits.filter((e) => e.status === "open").length} settling…` : `${fmt(p.stockAmount, 4)} ${meta.token}`}
            </span>
          </>
        ) : (
          <span className="mono dim">—</span>
        )}
      </td>
      <td className="r earn mono c-earn">{yours ? fmt(p.usdcEarned, 2) : "—"}</td>
      <td className="r">
        <span className="chev" aria-hidden="true">
          ›
        </span>
      </td>
    </tr>
  );
}

const HEADERS: { key: SortKey | null; label: string; cls?: string }[] = [
  { key: "asset", label: "Asset" },
  { key: "apy", label: "APY", cls: "r" },
  { key: null, label: "Funding, 7d", cls: "r c-spark" },
  { key: "position", label: "Position", cls: "r c-pos" },
  { key: "earned", label: "Earned (USDC)", cls: "r c-earn" },
  { key: null, label: "", cls: "r" },
];

export default function VaultTable() {
  const filter = useUiStore((s) => s.filter);
  const setFilter = useUiStore((s) => s.setFilter);
  const vaults = useVaultStore((s) => s.vaults);
  const positions = usePositionStore((s) => s.positions);
  const exits = usePositionStore((s) => s.exits);
  const status = useWalletStore((s) => s.status);
  const [sortKey, setSortKey] = useState<SortKey>("apy");
  const [dir, setDir] = useState<SortDir>("desc");
  const [search, setSearch] = useState("");

  const rows = useMemo(() => TICKERS.map((t): RowInput => ({ t, v: vaults[t], p: positions[t], exits: exits[t] })), [vaults, positions, exits]);
  const max = useMemo(() => sharedScale(rows.map((r) => sparkSeries(r.v.fundingSamples))), [rows]);
  const list = useMemo(() => sortRows(filterRows(rows, filter, search), sortKey, dir), [rows, filter, search, sortKey, dir]);

  function sortBy(key: SortKey) {
    if (key === sortKey) setDir(dir === "desc" ? "asc" : "desc");
    else {
      setSortKey(key);
      setDir(key === "asset" ? "asc" : "desc");
    }
  }

  return (
    <>
      <div className="th">
        <h2>
          <span className="sr">Carrera vaults. </span>Vaults <span className="mono">{TICKERS.length} markets · USDC yield on stock value</span>
        </h2>
        <div className="ctrl">
          <input className="search mono" type="search" placeholder="⌕ Search ticker" aria-label="Search ticker" value={search} onChange={(e) => setSearch(e.target.value)} />
          <div className="seg2" role="group" aria-label="Filter vaults">
            {SEGMENTS.map((f) => (
              <button key={f.key} className={filter === f.key ? "on" : ""} aria-pressed={filter === f.key} onClick={() => setFilter(f.key)}>
                {f.label}
              </button>
            ))}
          </div>
        </div>
      </div>
      <div className="rows-wrap">
        <table className="vt2">
          <thead>
            <tr>
              {HEADERS.map((h) => (
                <th key={h.label || "chev"} className={`${h.cls ?? ""}${h.key && sortKey === h.key ? " sort" : ""}`} aria-sort={h.key && sortKey === h.key ? (dir === "asc" ? "ascending" : "descending") : undefined}>
                  {h.key ? (
                    <button onClick={() => sortBy(h.key!)}>
                      {h.label}
                      {sortKey === h.key ? (dir === "desc" ? " ↓" : " ↑") : ""}
                    </button>
                  ) : (
                    h.label
                  )}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {list.length === 0 ? (
              <tr>
                <td colSpan={6} className="empty mono">
                  {filter === "positions" && status !== "connected" ? "Connect a wallet to see your positions." : filter === "positions" ? "No positions yet. Choose All to see every vault." : "No vault matches."}
                </td>
              </tr>
            ) : (
              list.map((r) => <Row key={r.t} r={r} max={max} />)
            )}
          </tbody>
        </table>
      </div>
      <div className="foot mono">
        <span>APY is an estimate from 24h average funding, net of borrow costs, before the 15% performance fee.</span>
        <span>Rates refresh hourly</span>
      </div>
    </>
  );
}
