"use client";

import { useId, useState, type KeyboardEvent, type MouseEvent } from "react";
import type { FundingSample } from "@/lib/types";
import { hourLabel, sparkPath, sparkSummary } from "@/lib/app2";
import { annualisedPct } from "@/lib/yield";

/**
 * 7-day funding line on a scale shared across the table (`max`), dashed zero line, dot on the last
 * point. Hover or focus a point (arrow keys move it) to read that hour's raw annualised funding and
 * its local time; the line itself stays 5-point smoothed. `raw` carries the unsmoothed samples.
 */
export default function Sparkline({ series, raw, max, funding, w = 110, h = 24 }: { series: number[]; raw: FundingSample[]; max: number; funding: boolean; w?: number; h?: number }) {
  const [active, setActive] = useState(-1);
  const id = useId();
  const g = sparkPath(series, max, w, h);
  const n = series.length;
  const cls = funding ? "spark on" : "spark";
  const sel = active >= 0 && active < n ? raw[active] : undefined;
  const pct = sel ? annualisedPct(sel.rateScaled) : 0;
  const xOf = (i: number) => (n > 1 ? (i / (n - 1)) * w : 0);
  const yOf = (i: number) => g.midY - (series[i] / max) * (g.midY - 2);

  function fromMouse(e: MouseEvent<SVGSVGElement>) {
    if (n === 0) return;
    const r = e.currentTarget.getBoundingClientRect();
    const k = Math.round(((e.clientX - r.left) / r.width) * (n - 1));
    setActive(Math.max(0, Math.min(n - 1, k)));
  }
  function onKey(e: KeyboardEvent) {
    if (n === 0) return;
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.preventDefault();
      setActive((i) => (i < 0 ? n - 1 : Math.max(0, Math.min(n - 1, i + (e.key === "ArrowLeft" ? -1 : 1)))));
    } else if (e.key === "Home") setActive(0);
    else if (e.key === "End") setActive(n - 1);
    else if (e.key === "Escape") setActive(-1);
  }

  return (
    <span
      className="spark-wrap"
      tabIndex={n ? 0 : -1}
      role="img"
      aria-label={sparkSummary(raw, annualisedPct)}
      aria-describedby={sel ? id : undefined}
      onKeyDown={onKey}
      onFocus={() => setActive((i) => (i < 0 ? n - 1 : i))}
      onBlur={() => setActive(-1)}
      onMouseLeave={() => setActive(-1)}
      onClick={(e) => e.stopPropagation()}
    >
      <svg className={cls} width={w} height={h} viewBox={`0 0 ${w} ${h}`} aria-hidden="true" onMouseMove={fromMouse}>
        <line x1="0" x2={w} y1={g.midY} y2={g.midY} className="zero" />
        {g.path && <path d={g.path} />}
        {sel ? (
          <>
            <line x1={xOf(active)} x2={xOf(active)} y1="0" y2={h} className="hair" />
            <circle cx={xOf(active)} cy={yOf(active)} r="2.6" className="pt" />
          </>
        ) : (
          g.last && <circle cx={g.last.x} cy={g.last.y} r="2.2" />
        )}
      </svg>
      {sel && (
        <span className="spark-tip mono" id={id} role="status" style={{ left: xOf(active) }}>
          <b className={pct < 0 ? "neg" : ""}>
            {pct < 0 ? "−" : ""}
            {Math.abs(pct).toFixed(1)}% a year
          </b>
          <em>{active === n - 1 ? "now" : hourLabel(sel.ts)}</em>
        </span>
      )}
    </span>
  );
}
