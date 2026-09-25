"use client";

import { useRef, useState } from "react";
import type { Ticker } from "@/constants/vaults";
import { fmt, pct } from "@/lib/format";
import { downsample, stateLabel } from "@/lib/ops-math";
import type { BookState } from "@/lib/ops-types";
import { useOpsStore } from "@/store/ops-provider";

const W = 600;
const H = 180;

const BAND: Record<BookState, string> = {
  basis: "var(--corsa)",
  parked: "url(#hatch-ops)",
  idle: "var(--line)",
  winding: "var(--ink)",
  unwinding: "var(--ink)",
  sizingup: "var(--corsa)",
  partialunwinding: "var(--corsa)",
};

/** Seven days of the 24h funding average against the hurdle, with the vault's state underneath. */
export default function FundingChart({ t }: { t: Ticker }) {
  const raw = useOpsStore((s) => s.history[t]);
  const b = useOpsStore((s) => s.books[t]);
  const svgRef = useRef<SVGSVGElement>(null);
  const [hover, setHover] = useState(-1);

  const pts = downsample(raw);
  const N = pts.length;
  const f = pts.map((p) => p.f_avg_bps);
  const h = pts.map((p) => p.hurdle_bps);
  const all = [...f, ...h, b.rule.enter_bps, b.rule.exit_bps].filter((v) => Number.isFinite(v));
  const lo = N ? Math.min(...all) : 0;
  const hi = N ? Math.max(...all) : 1;
  const pad = (hi - lo) * 0.15 || 100;
  const x = (i: number) => (N > 1 ? (i / (N - 1)) * W : 0);
  const y = (v: number) => H - ((v - (lo - pad)) / (hi + pad - (lo - pad))) * H;
  const path = (vals: number[]) => vals.map((v, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${y(v).toFixed(1)}`).join("");
  const bw = N ? W / N : W;
  const day = (ts: number) => new Date(ts * 1000).toLocaleDateString("en-US", { month: "short", day: "numeric" });
  const time = (ts: number) => new Date(ts * 1000).toLocaleString("en-US", { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });

  function move(e: React.PointerEvent<SVGSVGElement>) {
    const r = svgRef.current?.getBoundingClientRect();
    if (!r || N < 2) return;
    setHover(Math.max(0, Math.min(N - 1, Math.round(((e.clientX - r.left) / r.width) * (N - 1)))));
  }
  const tipLeft = hover >= 0 && svgRef.current ? Math.min(Math.max((hover / (N - 1)) * svgRef.current.clientWidth, 70), svgRef.current.clientWidth - 70) : 0;

  return (
    <section className="ops-panel" aria-label="Funding versus hurdle, 7 days" style={{ marginTop: 24 }}>
      <h3>Funding vs hurdle, 7 days</h3>
      <div className="cp-chart">
        <svg ref={svgRef} className="cp-svg" style={{ height: 180 }} viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" role="img" aria-label={`24h funding average over 7 days against a hurdle of ${pct(b.rule.hurdle_bps)}`} onPointerMove={move} onPointerDown={move} onPointerLeave={() => setHover(-1)}>
          <path d={`M0 ${H * 0.25}H${W}M0 ${H * 0.5}H${W}M0 ${H * 0.75}H${W}`} stroke="var(--line)" strokeWidth="1" vectorEffect="non-scaling-stroke" />
          {N > 1 && (
            <>
              <rect x="0" y={y(b.rule.enter_bps)} width={W} height={Math.max(0, y(b.rule.exit_bps) - y(b.rule.enter_bps))} fill="var(--ink)" opacity=".1" />
              <path d={path(h)} fill="none" stroke="var(--grey)" strokeWidth="1.5" strokeDasharray="4 3" vectorEffect="non-scaling-stroke" />
              <path d={path(f)} fill="none" stroke="var(--ink)" strokeWidth="2" vectorEffect="non-scaling-stroke" />
            </>
          )}
          {hover >= 0 && <line x1={x(hover)} x2={x(hover)} y1="0" y2={H} stroke="var(--ink)" strokeDasharray="3 3" vectorEffect="non-scaling-stroke" />}
        </svg>
        {hover >= 0 && (
          <div className="cp-tip" style={{ left: tipLeft }}>
            <span>{time(pts[hover].ts)}</span>
            <b className="num">
              {fmt(f[hover] / 100, 1)}% vs {fmt(h[hover] / 100, 1)}%
            </b>
            <em>{stateLabel[pts[hover].state]}</em>
          </div>
        )}
        <svg className="cp-band" viewBox={`0 0 ${W} 10`} preserveAspectRatio="none" aria-hidden="true">
          <defs>
            <pattern id="hatch-ops" width="5" height="5" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
              <rect width="5" height="5" fill="var(--paper)" />
              <rect width="2" height="5" fill="var(--line)" />
            </pattern>
          </defs>
          {pts.map((p, i) => (
            <rect key={i} x={(i * bw).toFixed(2)} y="0" width={(bw + 0.5).toFixed(2)} height="10" fill={BAND[p.state]} opacity={p.state === "idle" ? 0.35 : 1} />
          ))}
        </svg>
        <div className="cp-axis">
          <span>{N ? day(pts[0].ts) : ""}</span>
          <span>Now</span>
        </div>
      </div>
      <div className="cp-leg">
        <span>
          <b style={{ background: "var(--ink)", height: 2 }} />
          24h funding average
        </span>
        <span>
          <b style={{ background: "repeating-linear-gradient(90deg, var(--grey) 0 4px, transparent 4px 7px)", height: 2 }} />
          Hurdle
        </span>
        <span>
          <b style={{ background: "var(--ink)", opacity: 0.12 }} />
          Exit to enter band
        </span>
        <span>
          <b className="f" />
          Funding
        </span>
        <span>
          <b className="l" />
          Parked
        </span>
        <span>
          <b className="i" />
          Idle
        </span>
        <span>
          <b style={{ background: "var(--ink)" }} />
          Winding
        </span>
      </div>
    </section>
  );
}
