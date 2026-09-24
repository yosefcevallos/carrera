"use client";

import { useState } from "react";
import { VAULT_META, type Ticker } from "@/constants/vaults";
import { OPS_ACTION_LABEL, opsAction, type OpsActionKind } from "@/lib/chain/ops-actions";
import { fetchOps } from "@/lib/fetch-ops";
import { fmt, pct, usdCompact } from "@/lib/format";
import { ageLabel, basisBps, breaksFirst, fromE6, isLive, legDecimals, legSide, legVenueLabel, signedPct, signedQty, signedUsd, spotNotionalUsd, stateLabel, stateLong, units, USDC_DECIMALS } from "@/lib/ops-math";
import { useSigner } from "@/lib/use-signer";
import { useOpsStore } from "@/store/ops-provider";
import { useUiStore } from "@/store/ui-provider";

const ACTIONS: { kind: OpsActionKind; danger?: boolean; basisOnly?: boolean }[] = [
  { kind: "rebalance_to_kamino", basisOnly: true },
  { kind: "rebalance_to_phoenix", basisOnly: true },
  { kind: "unwind_emergency", danger: true, basisOnly: true },
  { kind: "pause", danger: true },
];

export default function BookView({ t }: { t: Ticker }) {
  const b = useOpsStore((s) => s.books[t]);
  const alerts = useOpsStore((s) => s.keeper.alerts);
  const setKeeper = useOpsStore((s) => s.setKeeper);
  const setBooks = useOpsStore((s) => s.setBooks);
  const showToast = useUiStore((s) => s.showToast);
  const signer = useSigner();
  const [busy, setBusy] = useState<OpsActionKind | "">("");
  const meta = VAULT_META[t];

  const live = isLive(b.state);
  const notional = spotNotionalUsd(b.legs);
  const basis = basisBps(b.legs);
  const first = breaksFirst(b.legs);
  const carryUsd = fromE6(b.carry.accrued_usdc_e6);
  const netUsd = fromE6(b.net_delta.usd_e6);
  const deltaWarn = notional > 0 && Math.abs(netUsd) / notional > 0.01;
  const liqWarn = first.leg !== undefined && first.distanceBps < 2000;
  const marginWarn = b.margin_bps !== null && b.margin_bps - b.min_margin_bps < 500;
  const ltvWarn = b.ltv_bps > 0 && b.emergency_ltv_bps - b.ltv_bps < 800;
  const mine = alerts.filter((a) => a.vault === t).sort((a, z) => z.ts - a.ts).slice(0, 4);

  async function run(kind: OpsActionKind) {
    setBusy(kind);
    try {
      const text = await opsAction(kind, t, signer);
      showToast(text);
      // Refresh in the background through the same fetcher and setters the syncer uses.
      fetchOps()
        .then((snap) => {
          setKeeper(snap.keeper);
          setBooks(snap.books);
        })
        .catch((err) => console.error("[BookView] ops refresh failed:", err));
    } catch (err) {
      showToast(err instanceof Error ? err.message : "That did not go through.");
    } finally {
      setBusy("");
    }
  }

  return (
    <>
      <header className="ops-head">
        <h2>{t}x basis</h2>
        <span className={`badge${live ? " live" : b.state === "parked" ? " mid" : ""}`}>{stateLabel[b.state]}</span>
        <span className="meta">
          {b.opened_ts ? `${stateLabel[b.state].toLowerCase()} ${ageLabel(b.opened_ts)}` : "not seen by the keeper yet"} · market {b.market_open ? "open" : "closed"}
          {b.pending_exit_shares > 0 ? ` · ${fmt(units(b.pending_exit_shares, 8), 2)} shares exiting in epoch ${b.epoch_id}` : ""}
        </span>
        <div className="ops-actions" role="group" aria-label="Manual cranks">
          {ACTIONS.map((a) => (
            <button key={a.kind} className={a.danger ? "danger" : ""} disabled={busy !== "" || (a.basisOnly && b.state !== "basis")} onClick={() => run(a.kind)}>
              {busy === a.kind ? "Sending…" : OPS_ACTION_LABEL[a.kind]}
            </button>
          ))}
        </div>
      </header>
      <p className="ops-sub">{stateLong[b.state]}. {meta.name}, tier {["A", "B", "C", "D"][b.tier] ?? "?"}, LTV {pct(meta.ltvBps, 0)}.</p>

      <dl className="ops-strip">
        <div className={deltaWarn ? "warn" : ""}>
          <dt>Net delta</dt>
          <dd className="num">{signedQty(units(b.net_delta.qty, 8))} sh</dd>
          <small>
            {signedUsd(netUsd)} of {usdCompact(notional)} basis
          </small>
        </div>
        <div>
          <dt>Carry PnL</dt>
          <dd className={`num${carryUsd > 0 ? " red" : ""}`}>{signedUsd(carryUsd)}</dd>
          <small>
            {signedPct(b.carry.ann_net_bps)} ann. net{b.carry.estimated ? ", estimated" : ""}
          </small>
        </div>
        <div>
          <dt>Basis</dt>
          <dd className="num">{signedPct(basis, 2)}</dd>
          <small>perp mark vs spot mark</small>
        </div>
        <div className={liqWarn ? "warn" : ""}>
          <dt>Liq distance</dt>
          <dd className="num">{first.leg ? (first.leg.kind === "short_perp" ? "+" : "−") + fmt(first.distanceBps / 100, 0) + "%" : "—"}</dd>
          <small>{first.leg ? `${first.leg.kind === "short_perp" ? "perp" : "Kamino"} leg breaks first` : "no leveraged leg"}</small>
        </div>
        {b.margin_bps !== null ? (
          <div className={marginWarn ? "warn" : ""}>
            <dt>Perp margin</dt>
            <dd className="num">{pct(b.margin_bps)}</dd>
            <small>min {pct(b.min_margin_bps)}, keeper tops up below {pct(b.min_margin_bps + 500)}</small>
          </div>
        ) : (
          <div className={ltvWarn ? "warn" : ""}>
            <dt>Kamino LTV</dt>
            <dd className="num">{pct(b.ltv_bps)}</dd>
            <small>
              emergency {pct(b.emergency_ltv_bps, 0)}, liq {pct(b.liq_ltv_bps, 0)}
            </small>
          </div>
        )}
      </dl>

      <div className="legs-wrap">
        <table className="legs">
          <thead>
            <tr>
              <th>Leg</th>
              <th>Venue</th>
              <th className="n">Size</th>
              <th className="n">Mark</th>
              <th className="n">Rate</th>
              <th className="n">Liq. price</th>
            </tr>
          </thead>
          <tbody>
            {b.legs.length === 0 && (
              <tr>
                <td colSpan={6} className="empty">
                  No open legs. {b.state === "idle" ? "The loan is repaid." : "Waiting for the keeper's first read."}
                </td>
              </tr>
            )}
            {b.legs.map((l) => {
              const dec = legDecimals(l.kind);
              const size = units(l.size, dec);
              const neg = l.kind === "borrow_usdc" || l.kind === "short_perp";
              const isFirst = first.leg === l;
              return (
                <tr key={l.kind} className={isFirst ? "first" : ""}>
                  <td>
                    <span className="side">{legSide[l.kind].side}</span> {l.kind === "supply_usdc" || l.kind === "borrow_usdc" ? "USDC" : `${t}${l.kind === "short_perp" ? "-PERP" : "x"}`}
                  </td>
                  <td className="venue">{legVenueLabel[l.kind]}</td>
                  <td className="n num">
                    {neg ? "−" : ""}
                    {fmt(size, dec === USDC_DECIMALS ? 0 : 2)}
                  </td>
                  <td className="n num">{fmt(fromE6(l.mark_e6), dec === USDC_DECIMALS ? 3 : 2)}</td>
                  <td className={`n num${l.rate_bps > 0 ? " red" : l.rate_bps < 0 ? " dim" : ""}`}>{l.rate_bps === 0 ? "0.0%" : signedPct(l.rate_bps)}</td>
                  <td className="n num">
                    {l.liq_price_e6 === null ? (
                      <span className="dim">—</span>
                    ) : l.kind === "borrow_usdc" ? (
                      <span className="dim">same leg</span>
                    ) : (
                      <>
                        {fmt(fromE6(l.liq_price_e6), 2)} <span className={isFirst ? "red" : "dim"}>({l.kind === "short_perp" ? "+" : "−"}{fmt((l.liq_distance_bps ?? 0) / 100, 0)}%)</span>
                      </>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>

      {mine.length > 0 && (
        <ul className="ops-alerts" aria-label={`${t} alerts`}>
          {mine.map((a, i) => (
            <li key={i} className={`ops-alert ${a.level}`}>
              <b>{a.message}</b>
              <span>{ageLabel(a.ts)}</span>
            </li>
          ))}
        </ul>
      )}
    </>
  );
}
