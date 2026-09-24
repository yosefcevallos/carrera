"use client";

import { TICKERS, VAULT_META, type Ticker } from "@/constants/vaults";
import Roundel from "@/components/Roundel";
import Wave from "@/components/Wave";
import { fmt, usd } from "@/lib/format";
import { modeLabel, modeLong, realisedYield } from "@/lib/yield";
import { usePositionStore } from "@/store/position-provider";
import { useUiStore, } from "@/store/ui-provider";
import { useVaultStore } from "@/store/vault-provider";
import { useWalletStore } from "@/store/wallet-provider";
import type { Filter } from "@/store/ui-store";

const FILTERS: { key: Filter; label: string }[] = [
  { key: "all", label: "All" },
  { key: "funding", label: "Funding" },
  { key: "yours", label: "Yours" },
];

function Row({ t }: { t: Ticker }) {
  const v = useVaultStore((s) => s.vaults[t]);
  const p = usePositionStore((s) => s.positions[t]);
  const e = usePositionStore((s) => s.pendingExits[t]);
  const open = useUiStore((s) => s.openVaultWindow);
  const meta = VAULT_META[t];
  const yours = p.shares > 0;
  const pending = e.shares > 0;
  const funding = v.mode === "funding";
  const y = realisedYield(v, 30);
  const openIt = () => open(t, yours || pending ? "withdraw" : "deposit");

  return (
    <tr
      className={`row${yours || pending ? " yours" : ""}`}
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
        <span className="vn">
          <Roundel n={meta.roundel} />
          <span>
            <b>{t}</b>
            <em>{meta.name}</em>
            <span className="mst">{modeLabel[v.mode]}</span>
          </span>
        </span>
      </td>
      <td className="c-st">
        <span className={`st${funding ? "" : " p"}`}>
          <i />
          {modeLabel[v.mode]}
        </span>
      </td>
      <td>
        <span className={`yl${funding ? "" : " p"}`}>
          <b className="num">{fmt(y.apy, 1)}%</b>
          <span>{y.sinceInception ? `since inception, ${y.days}d` : "realised, last 30d"}</span>
        </span>
      </td>
      <td className="c-wave">
        <Wave values={v.funding24h} />
      </td>
      <td className="dep num">
        {pending ? (
          <>
            <b>
              {fmt(e.stockAmount)} {meta.token}
            </b>
            <span>{e.ready ? "Ready to claim" : "Withdrawal settling…"}</span>
          </>
        ) : yours ? (
          <>
            <b>
              {fmt(p.stockAmount)} {meta.token}
            </b>
            <span>{p.usdcEarned >= 0 ? "+" : ""}{usd(p.usdcEarned, 2)} earned</span>
          </>
        ) : (
          <span className="muted">—</span>
        )}
      </td>
      <td>
        <span className="act">{pending && e.ready ? "Claim" : yours || pending ? "Manage" : "Deposit"}</span>
      </td>
    </tr>
  );
}

export default function VaultTable() {
  const filter = useUiStore((s) => s.filter);
  const setFilter = useUiStore((s) => s.setFilter);
  const vaults = useVaultStore((s) => s.vaults);
  const positions = usePositionStore((s) => s.positions);
  const pendingExits = usePositionStore((s) => s.pendingExits);
  const status = useWalletStore((s) => s.status);

  const list = TICKERS.filter((t) => {
    if (filter === "funding") return vaults[t].mode === "funding";
    if (filter === "yours") return positions[t].shares > 0 || pendingExits[t].shares > 0;
    return true;
  });

  return (
    <>
      <div className="gh">
        <div>
          <h1 style={{ fontSize: "inherit", fontWeight: "inherit" }}>
            <span className="sr">Carrera vaults. </span>
          </h1>
          <h2>Choose a stock to put to work</h2>
          <p>Every stock has its own vault. Rates update every hour.</p>
        </div>
        <div className="tog" role="group" aria-label="Filter vaults">
          {FILTERS.map((f) => (
            <button key={f.key} aria-pressed={filter === f.key} onClick={() => setFilter(f.key)}>
              {f.label}
            </button>
          ))}
        </div>
      </div>
      <div className="rows-wrap">
        <table className="vt">
          <thead>
            <tr>
              <th>Vault</th>
              <th className="c-st">Status</th>
              <th>Earned</th>
              <th className="c-wave">Funding, last 24h</th>
              <th>Your deposit</th>
              <th>
                <span className="sr">Action</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {list.length === 0 ? (
              <tr>
                <td colSpan={6} style={{ padding: 36, textAlign: "center", color: "var(--grey)" }}>
                  {status === "connected" ? "You haven't deposited yet. Choose All to see every vault." : "Connect a wallet to see your vaults."}
                </td>
              </tr>
            ) : (
              list.map((t) => <Row key={t} t={t} />)
            )}
          </tbody>
        </table>
      </div>
    </>
  );
}
