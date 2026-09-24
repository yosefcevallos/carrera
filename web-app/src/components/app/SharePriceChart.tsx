"use client";

import { useRef, useState } from "react";
import { VAULT_META, type Ticker } from "@/constants/vaults";
import { fmt, pct, usdCompact } from "@/lib/format";
import { bands, modeLabel, modeLong, trailingYield } from "@/lib/yield";
import type { ChartRange } from "@/store/ui-store";
import { useUiStore } from "@/store/ui-provider";
import { useVaultStore } from "@/store/vault-provider";

const W = 600;
const H = 200;
const RANGES: ChartRange[] = [7, 30, 90];

export default function SharePriceChart({ t }: { t: Ticker }) {
  const v = useVaultStore((s) => s.vaults[t]);
  const range = useUiStore((s) => s.range);
  const setRange = useUiStore((s) => s.setRange);
  const svgRef = useRef<SVGSVGElement>(null);
  const [hover, setHover] = useState(-1);
  const meta = VAULT_META[t];

  const pts = v.sharePriceHistory.slice(-(range + 1));
  const N = pts.length;
  const vals = pts.map((p) => p.usdcPerShare);
  const lo = N ? Math.min(...vals) : 0;
  const hi = N ? Math.max(...vals) : 0;
  const pad = (hi - lo) * 0.12 || 0.01;
  const x = (i: number) => (N > 1 ? (i / (N - 1)) * W : 0);
  const y = (val: number) => H - ((val - (lo - pad)) / (hi + pad - (lo - pad))) * H;
  const d = vals.map((val, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${y(val).toFixed(1)}`).join("");
  const bw = N ? W / N : W;
  const now = N ? vals[N - 1] : 0;
  const chg = N ? now - vals[0] : 0;
  const days = Math.max(0, N - 1);
  const yld = trailingYield(v.sharePriceHistory, range, v.priceUsd);
  const { enterBps, exitBps } = bands(v.hurdleBps, v.enterMarginBps, v.exitMarginBps);
  const fmtDay = (iso: string) => new Date(iso + "T00:00:00").toLocaleDateString("en-US", { month: "short", day: "numeric" });

  function move(e: React.PointerEvent<SVGSVGElement>) {
    const r = svgRef.current?.getBoundingClientRect();
    if (!r || N < 2) return;
    setHover(Math.max(0, Math.min(N - 1, Math.round(((e.clientX - r.left) / r.width) * (N - 1)))));
  }
  const tipLeft = hover >= 0 && svgRef.current ? Math.min(Math.max((hover / (N - 1)) * svgRef.current.clientWidth, 60), svgRef.current.clientWidth - 60) : 0;

  return (
    <>
      <div className="cp-top">
        <div>
          <span className="cp-l">Share price</span>
          <b className="cp-v num">
            1 {meta.token} {now >= 0 ? "+" : "−"} {fmt(Math.abs(now), 3)} USDC
          </b>
          <span className="cp-s">
            Each share is one {meta.token} plus the USDC it has earned. <b className="num">{chg >= 0 ? "+" : "−"}{fmt(Math.abs(chg), 3)} USDC</b> in {days} days
            {days > 0 ? `, ${fmt(yld.apy, 1)}% a year realised` : ""}.
          </span>
        </div>
        <div className="rng" role="group" aria-label="Chart range">
          {RANGES.map((n) => (
            <button key={n} aria-pressed={n === range} onClick={() => setRange(n)}>
              {n}D
            </button>
          ))}
        </div>
      </div>
      <div className="cp-chart">
        <svg
          ref={svgRef}
          className="cp-svg"
          viewBox={`0 0 ${W} ${H}`}
          preserveAspectRatio="none"
          role="img"
          aria-label={`USDC earned per share over ${days} days, from ${fmt(N ? vals[0] : 0, 3)} to ${fmt(now, 3)}`}
          onPointerMove={move}
          onPointerDown={move}
          onPointerLeave={() => setHover(-1)}
        >
          <path d={`M0 ${H * 0.25}H${W}M0 ${H * 0.5}H${W}M0 ${H * 0.75}H${W}`} stroke="var(--line)" strokeWidth="1" vectorEffect="non-scaling-stroke" />
          {N > 1 && (
            <>
              <path d={`${d}L${W},${H}L0,${H}Z`} fill="var(--ink)" opacity=".07" />
              <path d={d} fill="none" stroke="var(--ink)" strokeWidth="2" vectorEffect="non-scaling-stroke" />
            </>
          )}
          {hover >= 0 && <line x1={x(hover)} x2={x(hover)} y1="0" y2={H} stroke="var(--ink)" strokeDasharray="3 3" vectorEffect="non-scaling-stroke" />}
        </svg>
        {hover >= 0 && (
          <div className="cp-tip" style={{ left: tipLeft }}>
            <span>
              {fmtDay(pts[hover].date)}
              {hover === N - 1 ? " (today)" : ""}
            </span>
            <b className="num">{fmt(vals[hover], 3)} USDC</b>
            <em>{modeLabel[pts[hover].mode]}</em>
          </div>
        )}
        <svg className="cp-band" viewBox={`0 0 ${W} 10`} preserveAspectRatio="none" aria-hidden="true">
          <defs>
            <pattern id="hatch" width="5" height="5" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
              <rect width="5" height="5" fill="var(--paper)" />
              <rect width="2" height="5" fill="var(--line)" />
            </pattern>
          </defs>
          {pts.map((p, i) => (
            <rect key={i} x={(i * bw).toFixed(2)} y="0" width={(bw + 0.5).toFixed(2)} height="10" fill={p.mode === "funding" ? "var(--corsa)" : p.mode === "parked" ? "url(#hatch)" : "var(--line)"} opacity={p.mode === "idle" ? 0.35 : 1} />
          ))}
        </svg>
        <div className="cp-axis">
          <span>{N ? fmtDay(pts[0].date) : ""}</span>
          <span>Today</span>
        </div>
      </div>
      <div className="cp-leg">
        <span>
          <b className="f" />
          Earning from funding
        </span>
        <span>
          <b className="l" />
          Parked on Kamino
        </span>
        <span>
          <b className="i" />
          Idle
        </span>
      </div>
      <div className="hurdle" aria-label="Funding versus hurdle">
        <b>
          Funding 24h avg <span className={v.fundingAvgBps > v.hurdleBps ? "red" : ""}>{pct(v.fundingAvgBps)}</span> vs hurdle {pct(v.hurdleBps)}
        </b>
        <small>
          Enters funding above {pct(enterBps)}, exits below {pct(exitBps)}. The hurdle is what the loan would earn parked, plus its interest and a trading round trip. Read from chain, not a forecast.
        </small>
      </div>
      <dl className="cp-stats">
        <div>
          <dt>Realised, {range}d</dt>
          <dd className={`num${v.mode === "funding" ? " red" : ""}`}>
            {fmt(yld.apy, 1)}%<small>{yld.sinceInception ? `since inception, ${yld.days}d` : "a year, in USDC"}</small>
          </dd>
        </div>
        <div>
          <dt>Mode</dt>
          <dd>
            {modeLabel[v.mode]}
            <small>{v.mode === "funding" ? "hedged trade on Phoenix" : modeLong[v.mode]}</small>
          </dd>
        </div>
        <div>
          <dt>Vault size</dt>
          <dd className="num">
            {usdCompact(v.tvlUsd)}
            <small>of {usdCompact(v.capUsd)} cap</small>
          </dd>
        </div>
      </dl>
    </>
  );
}
