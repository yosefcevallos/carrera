// Display helpers for the ops dashboard. Pure functions over the keeper's wire types.
import { fmt } from "./format";
import type { BookState, Leg, LegKind, VaultBook } from "./ops-types";

/** The keeper does not send decimals; xStocks are 8, USDC is 6 (keeper.example.toml). */
export const STOCK_DECIMALS = 8;
export const USDC_DECIMALS = 6;

export const fromE6 = (v: number) => v / 1e6;
export const units = (size: number, decimals: number) => size / 10 ** decimals;

export const legDecimals = (kind: LegKind) => (kind === "long_spot" || kind === "short_perp" ? STOCK_DECIMALS : USDC_DECIMALS);

/** "+0.40 sh" style; the sign is always shown. */
export function signedQty(qty: number, d = 2): string {
  return (qty > 0 ? "+" : qty < 0 ? "−" : "") + fmt(Math.abs(qty), d);
}

export function signedUsd(usd: number): string {
  const abs = Math.abs(usd);
  const body = abs >= 1000 ? fmt(abs, 0) : fmt(abs, 2);
  return (usd > 0 ? "+" : usd < 0 ? "−" : "") + "$" + body;
}

export function signedPct(bps: number, d = 1): string {
  return (bps > 0 ? "+" : bps < 0 ? "−" : "") + fmt(Math.abs(bps) / 100, d) + "%";
}

/** "19d ago", "3h ago", "22m ago"; "—" before the keeper has seen the vault. */
export function ageLabel(openedTs: number, nowSec = Date.now() / 1000): string {
  if (!openedTs) return "—";
  const s = Math.max(0, nowSec - openedTs);
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86_400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86_400)}d ago`;
}

export const stateLabel: Record<BookState, string> = {
  basis: "Funding",
  parked: "Parked",
  idle: "Idle",
  winding: "Winding in",
  unwinding: "Unwinding",
  sizingup: "Sizing up",
  partialunwinding: "Releasing",
};

export const stateLong: Record<BookState, string> = {
  basis: "Delta neutral: spot long on Kamino, short perp on Phoenix",
  parked: "USDC supplied on Kamino",
  idle: "Loan repaid, waiting for funding",
  winding: "Moving the loan into the basis trade",
  unwinding: "Closing the basis trade",
  sizingup: "Adding new deposits to the basis trade",
  partialunwinding: "Releasing part of the basis trade for exits",
};

export const legSide: Record<LegKind, { side: string; what: string }> = {
  long_spot: { side: "Long", what: "spot" },
  borrow_usdc: { side: "Borrow", what: "USDC" },
  supply_usdc: { side: "Supply", what: "USDC" },
  short_perp: { side: "Short", what: "perp" },
};

export const legVenueLabel: Record<LegKind, string> = {
  long_spot: "Pledged, Kamino",
  borrow_usdc: "Kamino",
  supply_usdc: "Kamino",
  short_perp: "Phoenix",
};

/** Perp mark versus spot mark, bps. 0 when either leg is absent. */
export function basisBps(legs: Leg[]): number {
  const spot = legs.find((l) => l.kind === "long_spot");
  const perp = legs.find((l) => l.kind === "short_perp");
  if (!spot || !perp || !spot.mark_e6) return 0;
  return Math.round(((perp.mark_e6 - spot.mark_e6) * 10_000) / spot.mark_e6);
}

/** Spot notional in USD of the basis legs (the depositor's stock is excluded). */
export function spotNotionalUsd(legs: Leg[]): number {
  const spot = legs.find((l) => l.kind === "long_spot");
  if (!spot) return 0;
  return units(spot.size, STOCK_DECIMALS) * fromE6(spot.mark_e6);
}

/** The leg with the smallest liquidation distance, or none when no leg can liquidate. */
export function breaksFirst(legs: Leg[]): { leg: Leg | undefined; distanceBps: number } {
  let best: Leg | undefined;
  for (const l of legs) {
    if (l.liq_distance_bps === null) continue;
    if (!best || l.liq_distance_bps < (best.liq_distance_bps as number)) best = l;
  }
  return { leg: best, distanceBps: best ? (best.liq_distance_bps as number) : 0 };
}

/** Which of the two health thresholds the keeper alerts on is closest, in bps. */
export function ltvHeadroomBps(b: VaultBook): number {
  return b.emergency_ltv_bps - b.ltv_bps;
}

export function marginHeadroomBps(b: VaultBook): number {
  return b.margin_bps === null ? 0 : b.margin_bps - b.min_margin_bps;
}

export function isLive(state: BookState): boolean {
  return state === "basis" || state === "winding" || state === "unwinding" || state === "sizingup" || state === "partialunwinding";
}

/** Sample every n-th point so an SVG path stays under ~600 nodes. */
export function downsample<T>(points: T[], max = 600): T[] {
  if (points.length <= max) return points;
  const step = Math.ceil(points.length / max);
  const out: T[] = [];
  for (let i = 0; i < points.length; i += step) out.push(points[i]);
  if (out[out.length - 1] !== points[points.length - 1]) out.push(points[points.length - 1]);
  return out;
}
