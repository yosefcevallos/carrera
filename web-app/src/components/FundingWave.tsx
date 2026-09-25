"use client";

import { useId, useState, type KeyboardEvent } from "react";
import type { FundingSample } from "@/lib/types";
import { annualisedPct } from "@/lib/yield";

const BAR = 14;
const GAP = 4;
const H = 30;
const DAYS = 7;
const DAY_MS = 86_400_000;
/** A small day still registers. */
const MIN_BAR = 2;

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

/**
 * Mean per UTC day for exactly the `days` most recent days ending today (oldest first). Days with
 * no samples are absent; an older partial day outside the window is dropped rather than shown.
 */
export function byDay(samples: FundingSample[], days = DAYS, now = Date.now()): DayBar[] {
  const today = Math.floor(now / DAY_MS) * DAY_MS;
  const oldest = today - (days - 1) * DAY_MS;
  const acc = new Map<number, { sum: number; count: number }>();
  for (const s of samples) {
    const day = Math.floor(s.ts / DAY_MS) * DAY_MS;
    if (day < oldest || day > today) continue;
    const a = acc.get(day) ?? { sum: 0, count: 0 };
    a.sum += s.rateScaled;
    a.count += 1;
    acc.set(day, a);
  }
  return [...acc.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([day, a]) => ({ day, rateScaled: Math.round(a.sum / a.count), count: a.count, partial: day === today }));
}

export interface BarGeometry {
  /** y of the baseline inside the band */
  baselineY: number;
  rects: { y: number; height: number; neg: boolean }[];
}

/**
 * Baseline at the bottom when every value is non-negative; at the vertical middle when any is
 * negative, positives drawn up and negatives down, all scaled by max |value|. Min height MIN_BAR.
 */
export function layoutBars(values: number[], height = H): BarGeometry {
  const anyNeg = values.some((v) => v < 0);
  const baselineY = anyNeg ? height / 2 : height;
  const room = anyNeg ? height / 2 : height;
  const max = Math.max(1, ...values.map((v) => Math.abs(v)));
  const rects = values.map((v) => {
    const h = Math.max(MIN_BAR, (Math.abs(v) / max) * room);
    const neg = v < 0;
    return { y: neg ? baselineY : baselineY - h, height: h, neg };
  });
  return { baselineY, rects };
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
 * Seven daily bars, newest on the right. Positive means in ink, negative in grey below the
 * baseline. Hover or focus and use the arrow keys to read one day's annualised mean.
 */
export default function FundingWave({ samples }: { samples: FundingSample[] }) {
  const [active, setActive] = useState(-1);
  const id = useId();
  const bars = byDay(samples);
  const n = bars.length;
  if (n === 0) return <span className="fwave-empty">—</span>;
  const w = DAYS * (BAR + GAP) - GAP;
  const offset = (DAYS - n) * (BAR + GAP); // right-align when fewer than 7 days exist
  const geo = layoutBars(bars.map((b) => b.rateScaled));
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
        <line x1="0" x2={w} y1={geo.baselineY} y2={geo.baselineY} className="fwave-base" />
        {bars.map((b, i) => {
          const r = geo.rects[i];
          return (
            <rect
              key={b.day}
              x={offset + i * (BAR + GAP)}
              width={BAR}
              y={r.y}
              height={r.height}
              className={`${r.neg ? "neg" : "pos"}${i === active ? " on" : ""}`}
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
