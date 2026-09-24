"use client";

import type { Ticker } from "@/constants/vaults";
import { pct } from "@/lib/format";
import { useOpsStore } from "@/store/ops-provider";

const DECISION: Record<string, string> = {
  none: "hold",
  to_basis: "enter funding",
  to_parked: "park on Kamino",
  to_idle: "repay to idle",
};

function Gauge({ label, value, max, marks, note }: { label: string; value: number; max: number; marks: { at: number; hard?: boolean; label: string }[]; note: string }) {
  const p = (v: number) => `${Math.max(0, Math.min(100, (v / max) * 100))}%`;
  return (
    <div className="gauge">
      <div className="lab">
        <span>{label}</span>
        <b className="num">{pct(value)}</b>
      </div>
      <div className="bar2" role="img" aria-label={`${label} ${pct(value)}, ${marks.map((m) => `${m.label} ${pct(m.at)}`).join(", ")}`}>
        <div className="fill" style={{ width: p(value) }} />
        {marks.map((m) => (
          <div key={m.label} className={`mark${m.hard ? "" : " soft"}`} style={{ left: p(m.at) }} title={`${m.label} ${pct(m.at)}`} />
        ))}
      </div>
      <small>{note}</small>
    </div>
  );
}

export default function RuleAndHealth({ t }: { t: Ticker }) {
  const b = useOpsStore((s) => s.books[t]);
  const r = b.rule;
  const above = r.f_avg_bps > r.hurdle_bps;

  return (
    <div className="ops-two">
      <section className="ops-panel" aria-label="Allocation rule">
        <h3>Rule</h3>
        <p className="rule-line">
          Funding 24h avg <b className={`num${above ? " red" : ""}`}>{pct(r.f_avg_bps)}</b> vs hurdle <b className="num">{pct(r.hurdle_bps)}</b>
        </p>
        <dl className="kv">
          <div>
            <dt>Enters funding above</dt>
            <dd className="num">{pct(r.enter_bps)}</dd>
          </div>
          <div>
            <dt>Exits below</dt>
            <dd className="num">{pct(r.exit_bps)}</dd>
          </div>
          <div>
            <dt>Parked yield (Kamino supply)</dt>
            <dd className="num">{pct(r.parked_apy_bps)}</dd>
          </div>
          <div>
            <dt>Borrow rate</dt>
            <dd className="num">{pct(r.r_bps)}</dd>
          </div>
          <div>
            <dt>Samples</dt>
            <dd className="num">{r.samples} / 24</dd>
          </div>
          <div>
            <dt>Decision now</dt>
            <dd className={r.decision === "none" ? "" : "red"}>{DECISION[r.decision] ?? r.decision}</dd>
          </div>
        </dl>
        <small>The hurdle is what the loan would earn parked, plus the secondary loan's interest and a trading round trip amortised over a month. The program evaluates the same rule on-chain; the keeper only cranks what it permits.</small>
      </section>
      <section className="ops-panel" aria-label="Health">
        <h3>Health</h3>
        <Gauge label="Kamino LTV" value={b.ltv_bps} max={Math.max(b.liq_ltv_bps, 1)} marks={[{ at: b.emergency_ltv_bps, label: "emergency" }, { at: b.liq_ltv_bps, hard: true, label: "liquidation" }]} note={`Keeper repays from Phoenix above ${pct(Math.min(b.liq_ltv_bps, b.ltv_bps + 800), 0)}; emergency unwind at ${pct(b.emergency_ltv_bps, 0)}.`} />
        {b.margin_bps !== null ? (
          <Gauge label="Phoenix margin" value={b.margin_bps} max={Math.max(b.margin_bps, b.min_margin_bps) * 1.5 || 1} marks={[{ at: b.min_margin_bps, hard: true, label: "minimum" }, { at: b.min_margin_bps + 500, label: "top-up" }]} note={`Keeper tops up below ${pct(b.min_margin_bps + 500)}; emergency below ${pct(b.min_margin_bps)}.`} />
        ) : (
          <p className="dim" style={{ fontSize: 13, marginTop: 14 }}>
            No Phoenix position, so no perp margin to watch.
          </p>
        )}
        <dl className="kv" style={{ marginTop: 14 }}>
          <div>
            <dt>NAV</dt>
            <dd className="num">${(b.nav_usd_e6 / 1e6).toLocaleString("en-US", { maximumFractionDigits: 0 })}</dd>
          </div>
          <div>
            <dt>Share price</dt>
            <dd className="num">{(b.share_price_stock_e6 / 1e6).toFixed(4)} stock</dd>
          </div>
          <div>
            <dt>NAV age</dt>
            <dd className={`num${b.nav_age_slots > 150 ? " red" : ""}`}>{b.nav_age_slots} slots</dd>
          </div>
        </dl>
      </section>
    </div>
  );
}
