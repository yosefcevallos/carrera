"use client";

import { useId, useState, type KeyboardEvent } from "react";
import type { FundingSample } from "@/lib/types";
import { annualisedPct } from "@/lib/yield";

const BAR = 14;
const GAP = 4;
const H = 30;
const DAYS = 7;
const DAY_MS = 86_400_000;

export interface DayBar {
  /** UTC midnight of the day, unix ms */
  day: number;
  /** Mean of the day's hourly samples, in the program's scaled unit */
  rateScaled: number;
  /** Number of hourly samples in the mean */
  count: number;
  /** True for the newest day when it is still in progress */
  partial: boolean;
}

/** Group hourly samples by UTC day, mean per day, oldest first, at most the newest `days`. */
export function byDay(samples: FundingSample[], days = DAYS, now = Date.now()): DayBar[] {
  const acc = new Map<number, { sum: number; count: number }>();
  for (const s of samples) {
    const day = Math.floor(s.ts / DAY_MS) * DAY_MS;
    const a = acc.get(day) ?? { sum: 0, count: 0 };
    a.sum += s.rateScaled;
    a.count += 1;
    acc.set(day, a);
  }
  const today = Math.floor(now / DAY_MS) * DAY_MS;
  return [...acc.entries()]
    .sort((a, b) => a[0] - b[0])
    .slice(-days)
    .map(([day, a]) => ({ day, rateScaled: Math.round(a.sum / a.count), count: a.count, partial: day === today }));
}

/** "Thu", or "Today so far" for the partial newest day. Weekday in UTC to match the grouping. */
export function dayLabel(bar: DayBar): string {
  if (bar.partial) return "Today so far";
  return new Date(bar.day).toLocaleString(undefined, { weekday: "short", timeZone: "UTC" });
}

export function countLabel(bar: DayBar): string {
  return bar.partial ? `${bar.count} so far` : `${bar.count} hourly sample${bar.count === 1 ? "" : "s"}`;
}

/**
 * Seven daily bars, newest on the right. Positive means in ink above the baseline, negative in
 * grey below it. Hover or focus and use the arrow keys to read one day's annualised mean.
 */
export default function FundingWave({ samples }: { samples: FundingSample[] }) {
  const [active, setActive] = useState(-1);
  const id = useId();
  const bars = byDay(samples);
  const n = bars.length;
  if (n === 0) return <span className="fwave-empty">—</span>;
  const w = DAYS * (BAR + GAP) - GAP;
  const offset = (DAYS - n) * (BAR + GAP); // right-align when fewer than 7 days exist
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
      aria-label={`Daily average funding, last ${n} day${n === 1 ? "" : "s"}`}
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
              key={b.day}
              x={offset + i * (BAR + GAP)}
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
        <span className="fwave-tip" id={id} role="status" style={{ left: offset + (active + 0.5) * (BAR + GAP) }}>
          <b className={pct < 0 ? "neg" : ""}>
            {dayLabel(sel)}: {pct < 0 ? "−" : ""}
            {Math.abs(pct).toFixed(1)}% a year
          </b>
          <em>{countLabel(sel)}</em>
        </span>
      )}
    </span>
  );
}
