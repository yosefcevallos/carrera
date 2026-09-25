"use client";

import { useId, useState, type KeyboardEvent } from "react";
import type { FundingSample } from "@/lib/types";
import { annualisedPct } from "@/lib/yield";

const BAR = 2;
const GAP = 1;
const H = 30;

/** "Wed 3 pm" in the viewer's local time. */
export function sampleLabel(tsMs: number): string {
  return new Date(tsMs).toLocaleString(undefined, { weekday: "short", hour: "numeric" }).replace(/\s?([AP]M)$/, (_, m: string) => " " + m.toLowerCase());
}

/**
 * Hourly funding bars, newest on the right. Positive rates in ink above the baseline, negative in
 * grey below it. Hover or focus and use the arrow keys to read one hour's annualised rate.
 */
export default function FundingWave({ samples }: { samples: FundingSample[] }) {
  const [active, setActive] = useState(-1);
  const id = useId();
  const n = samples.length;
  if (n === 0) return <span className="fwave-empty">—</span>;
  const w = n * (BAR + GAP) - GAP;
  const max = Math.max(1, ...samples.map((s) => Math.abs(s.rateScaled)));
  const mid = H / 2;
  const sel = active >= 0 && active < n ? samples[active] : undefined;
  const pct = sel ? annualisedPct(sel.rateScaled) : 0;

  function onKey(e: KeyboardEvent) {
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.preventDefault();
      setActive((i) => (i < 0 ? n - 1 : Math.max(0, Math.min(n - 1, i + (e.key === "ArrowLeft" ? -1 : 1)))));
    } else if (e.key === "Home") setActive(0);
    else if (e.key === "End") setActive(n - 1);
    else if (e.key === "Escape") setActive(-1);
  }

  return (
    <span
      className="fwave"
      tabIndex={0}
      role="img"
      aria-label={`Hourly funding, last ${n} hours`}
      aria-describedby={sel ? id : undefined}
      onKeyDown={onKey}
      onFocus={() => setActive((i) => (i < 0 ? n - 1 : i))}
      onBlur={() => setActive(-1)}
      onMouseLeave={() => setActive(-1)}
      onClick={(e) => e.stopPropagation()}
    >
      <svg viewBox={`0 0 ${w} ${H}`} preserveAspectRatio="none" style={{ maxWidth: w }} aria-hidden="true">
        <line x1="0" x2={w} y1={mid} y2={mid} className="fwave-base" />
        {samples.map((s, i) => {
          const h = Math.max(1, (Math.abs(s.rateScaled) / max) * (mid - 1));
          const x = i * (BAR + GAP);
          const neg = s.rateScaled < 0;
          return (
            <rect
              key={i}
              x={x}
              width={BAR}
              y={neg ? mid : mid - h}
              height={h}
              className={`${neg ? "neg" : "pos"}${i === active ? " on" : ""}`}
              onMouseEnter={() => setActive(i)}
            />
          );
        })}
      </svg>
      {sel && (
        <span className="fwave-tip" id={id} role="status" style={{ left: `${((active + 0.5) / n) * 100}%` }}>
          <b className={pct < 0 ? "neg" : ""}>{pct < 0 ? "−" : ""}{Math.abs(pct).toFixed(1)}% a year</b>
          <em>{sampleLabel(sel.ts)}</em>
        </span>
      )}
    </span>
  );
}
