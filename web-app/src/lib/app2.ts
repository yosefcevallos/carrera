// Pure helpers for the fintech app page (docs/frontend-handoff/app-fintech-v1.html): sparklines on
// a shared scale, table sort and filter, percentage chips in raw units, and the status-line clocks.
import { TICKERS, type Ticker } from "@/constants/vaults";
import type { FundingSample, Position, VaultExit, VaultRecord } from "./types";
import { currentApyBps } from "./yield";
import { pendingShares } from "./exits";

export const HOUR_MS = 3_600_000;

/** Centred 5-point moving mean, like the reference; edges use the samples that exist. */
export function smooth5(values: number[]): number[] {
  return values.map((_, i) => {
    const w = values.slice(Math.max(0, i - 2), i + 3);
    return w.reduce((a, b) => a + b, 0) / w.length;
  });
}

/** The 28 most recent hourly samples, smoothed, in the program's scaled unit (oldest first). */
export function sparkSeries(samples: FundingSample[], points = 28): number[] {
  const tail = samples.slice(-points).map((s) => s.rateScaled);
  return smooth5(tail);
}

/** The same 28 samples unsmoothed, so a tooltip can show the raw hourly value behind each point. */
export function sparkRaw(samples: FundingSample[], points = 28): FundingSample[] {
  return samples.slice(-points);
}

/** "Thu 3 pm" in the viewer's local time. */
export function hourLabel(ts: number): string {
  return new Date(ts)
    .toLocaleString(undefined, { weekday: "short", hour: "numeric" })
    .replace(",", "")
    .replace(/\s?([AP]M)$/i, (m) => m.toLowerCase());
}

/** "5 seven-day funding: 12.1% to 34.8% a year" — the accessible summary of a sparkline. */
export function sparkSummary(raw: FundingSample[], annualise: (r: number) => number): string {
  if (raw.length === 0) return "No funding samples yet";
  const pcts = raw.map((r) => annualise(r.rateScaled));
  const lo = Math.min(...pcts);
  const hi = Math.max(...pcts);
  return `Hourly funding over 7 days, ${lo.toFixed(1)}% to ${hi.toFixed(1)}% a year annualised, ${raw.length} samples`;
}

/** Max |value| across every vault's series so all sparklines share one vertical scale. */
export function sharedScale(series: number[][]): number {
  const m = Math.max(0, ...series.flatMap((s) => s.map(Math.abs)));
  return m > 0 ? m : 1;
}

export interface SparkGeometry {
  path: string;
  midY: number;
  last: { x: number; y: number } | null;
}

/** SVG path for a series on a `w`×`h` box with the zero line at mid height, scaled by `max`. */
export function sparkPath(series: number[], max: number, w = 110, h = 24): SparkGeometry {
  const mid = h / 2;
  if (series.length === 0) return { path: "", midY: mid, last: null };
  const n = Math.max(1, series.length - 1);
  const x = (i: number) => (i / n) * w;
  const y = (v: number) => mid - (v / max) * (mid - 2);
  const path = series.map((v, i) => `${i ? "L" : "M"}${x(i).toFixed(1)},${y(v).toFixed(1)}`).join("");
  const li = series.length - 1;
  return { path, midY: mid, last: { x: x(li), y: y(series[li]) } };
}

export type SortKey = "apy" | "asset" | "position" | "earned";
export type SortDir = "asc" | "desc";
export type Segment = "all" | "funding" | "positions";

export interface RowInput {
  t: Ticker;
  v: VaultRecord;
  p: Position;
  exits: VaultExit[];
}

export const apyOf = (r: RowInput) => currentApyBps(r.v);
export const positionUsd = (r: RowInput) => r.p.stockAmount * r.v.priceUsd;

/** Sort rows by a column; ties fall back to ticker order so the list is stable. */
export function sortRows(rows: RowInput[], key: SortKey, dir: SortDir): RowInput[] {
  const sign = dir === "asc" ? 1 : -1;
  const val = (r: RowInput): number | string =>
    key === "apy" ? apyOf(r) : key === "position" ? positionUsd(r) : key === "earned" ? r.p.usdcEarned : r.t;
  return [...rows].sort((a, b) => {
    const va = val(a);
    const vb = val(b);
    const c = typeof va === "string" ? va.localeCompare(vb as string) : va - (vb as number);
    return c !== 0 ? sign * c : TICKERS.indexOf(a.t) - TICKERS.indexOf(b.t);
  });
}

/** Segment control plus ticker search. Search matches the ticker prefix, case-insensitive. */
export function filterRows(rows: RowInput[], segment: Segment, search: string): RowInput[] {
  const q = search.trim().toUpperCase();
  return rows.filter((r) => {
    if (segment === "funding" && r.v.mode !== "funding") return false;
    if (segment === "positions" && !(r.p.shares > 0 || pendingShares(r.exits) > 0)) return false;
    return !q || r.t.startsWith(q);
  });
}

/** `pct` percent of a raw balance, floored to whole base units so it never exceeds what is held. */
export function chipRaw(balanceRaw: bigint, pct: number): bigint {
  if (pct >= 100) return balanceRaw;
  return (balanceRaw * BigInt(Math.round(pct))) / 100n;
}

/** Top of the next hour after `now` (unix ms). */
export function nextTopOfHour(now: number): number {
  return Math.floor(now / HOUR_MS) * HOUR_MS + HOUR_MS;
}

/** "mm:ss" until `target`; clamps at 00:00. Hours roll into minutes ("72:05"). */
export function formatCountdown(msLeft: number): string {
  const s = Math.max(0, Math.floor(msLeft / 1000));
  const m = Math.floor(s / 60);
  return `${String(m).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

/** "23:00 UTC" for a unix-ms timestamp, floored to the hour. */
export function hourUtc(ts: number): string {
  const d = new Date(Math.floor(ts / HOUR_MS) * HOUR_MS);
  return `${String(d.getUTCHours()).padStart(2, "0")}:00 UTC`;
}

/** Annualised percent of the newest hourly sample; 0 when there are no samples. */
export function fundingNowPct(samples: FundingSample[], annualise: (r: number) => number): number {
  return samples.length ? annualise(samples[samples.length - 1].rateScaled) : 0;
}

/** Annualised percent of the mean of the newest 3 hourly samples (D8 entry average); null with fewer than 3. */
export function funding3hPct(samples: FundingSample[], annualise: (r: number) => number): number | null {
  if (samples.length < 3) return null;
  const last = samples.slice(-3);
  return annualise(last.reduce((a, s) => a + s.rateScaled, 0) / 3);
}

/** The most recent funding sample across all vaults, unix ms; 0 when none. */
export function lastFundingTs(vaults: Record<Ticker, VaultRecord>): number {
  let last = 0;
  for (const t of TICKERS) for (const s of vaults[t].fundingSamples) if (s.ts > last) last = s.ts;
  return last;
}

/** "just now", "2m ago", "1h ago"; "—" before the first sync. */
export function agoLabel(since: number, now: number): string {
  if (!since) return "—";
  const s = Math.max(0, Math.floor((now - since) / 1000));
  if (s < 45) return "just now";
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  return `${Math.floor(m / 60)}h ago`;
}

/** "4pQz…9LmT" for a transaction signature; empty string stays empty. */
export function shortSig(sig: string): string {
  return sig.length > 12 ? `${sig.slice(0, 4)}…${sig.slice(-4)}` : sig;
}

/** Position-weighted current APY across held positions, bps. */
export function weightedApyBps(vaults: Record<Ticker, VaultRecord>, positions: Record<Ticker, Position>): number {
  let value = 0;
  let acc = 0;
  for (const t of TICKERS) {
    const usd = positions[t].stockAmount * vaults[t].priceUsd;
    if (usd <= 0) continue;
    value += usd;
    acc += usd * currentApyBps(vaults[t]);
  }
  return value > 0 ? Math.round(acc / value) : 0;
}
