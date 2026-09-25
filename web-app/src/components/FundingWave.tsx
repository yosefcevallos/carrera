"use client";

import { useEffect, useId, useState, type KeyboardEvent } from "react";
import type { FundingSample } from "@/lib/types";
import { annualisedPct } from "@/lib/yield";

const BAR = 3;
const GAP = 1;
const H = 30;
/** Below this viewport width the 168 hourly samples are bucketed into 3-hour means. */
const BUCKET_BELOW_PX = 1100;
const BUCKET_HOURS = 3;

export interface Bar {
  /** Start of the bar's period, unix ms */
  ts: number;
  /** Hours covered: 1 for a raw sample, 3 for a bucket */
  hours: number;
  rateScaled: number;
}

/** Mean of each `hours`-sized bucket, oldest first; a trailing partial bucket is kept. */
export function bucket(samples: FundingSample[], hours: number): Bar[] {
  if (hours <= 1) return samples.map((s) => ({ ts: s.ts, hours: 1, rateScaled: s.rateScaled }));
  const out: Bar[] = [];
  for (let i = 0; i < samples.length; i += hours) {
    const slice = samples.slice(i, i + hours);
    out.push({ ts: slice[0].ts, hours: slice.length, rateScaled: Math.round(slice.reduce((a, s) => a + s.rateScaled, 0) / slice.length) });
  }
  return out;
}

const hourLabel = (d: Date) => d.toLocaleString(undefined, { hour: "numeric" }).replace(/\s?([AP]M)$/i, (_, m: string) => " " + m.toLowerCase());

/** "Wed 3 pm" for one hour, "Wed 3–6 pm" for a bucket, in the viewer's local time. */
export function barLabel(bar: Bar): string {
  const start = new Date(bar.ts);
  const day = start.toLocaleString(undefined, { weekday: "short" });
  if (bar.hours <= 1) return `${day} ${hourLabel(start)}`;
  const end = new Date(bar.ts + bar.hours * 3_600_000);
  const a = hourLabel(start);
  const b = hourLabel(end);
  const sameMeridiem = a.slice(-2) === b.slice(-2);
  return `${day} ${sameMeridiem ? a.replace(/ [ap]m$/, "") : a}–${b}`;
}

function useNarrow(): boolean {
  const [narrow, setNarrow] = useState(false);
  useEffect(() => {
    const mq = matchMedia(`(max-width: ${BUCKET_BELOW_PX - 1}px)`);
    const on = () => setNarrow(mq.matches);
    on();
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, []);
  return narrow;
}

/**
 * Funding bars, newest on the right. Positive rates in ink above the baseline, negative in grey
 * below it. Hover or focus and use the arrow keys to read one bar's annualised rate.
 */
export default function FundingWave({ samples }: { samples: FundingSample[] }) {
  const [active, setActive] = useState(-1);
  const id = useId();
  const narrow = useNarrow();
  const bars = bucket(samples, narrow ? BUCKET_HOURS : 1);
  const n = bars.length;
  if (n === 0) return <span className="fwave-empty">—</span>;
  const w = n * (BAR + GAP) - GAP;
  const max = Math.max(1, ...bars.map((b) => Math.abs(b.rateScaled)));
  const mid = H / 2;
  const sel = active >= 0 && active < n ? bars[active] : undefined;
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
      aria-label={`Funding, last 7 days, ${n} bars`}
      aria-describedby={sel ? id : undefined}
      onKeyDown={onKey}
      onFocus={() => setActive((i) => (i < 0 ? n - 1 : i))}
      onBlur={() => setActive(-1)}
      onMouseLeave={() => setActive(-1)}
      onClick={(e) => e.stopPropagation()}
    >
      <svg width={w} height={H} viewBox={`0 0 ${w} ${H}`} aria-hidden="true">
        <line x1="0" x2={w} y1={mid} y2={mid} className="fwave-base" />
        {bars.map((b, i) => {
          const h = Math.max(1, (Math.abs(b.rateScaled) / max) * (mid - 1));
          const neg = b.rateScaled < 0;
          return (
            <rect
              key={i}
              x={i * (BAR + GAP)}
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
        <span className="fwave-tip" id={id} role="status" style={{ left: (active + 0.5) * (BAR + GAP) }}>
          <b className={pct < 0 ? "neg" : ""}>
            {pct < 0 ? "−" : ""}
            {Math.abs(pct).toFixed(1)}% a year{sel.hours > 1 ? ", 3h mean" : ""}
          </b>
          <em>{barLabel(sel)}</em>
        </span>
      )}
    </span>
  );
}
