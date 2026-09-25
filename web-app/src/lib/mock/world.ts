// Deterministic in-memory world for the mock data source. Shapes match docs/frontend-handoff/vaults.sample.json
// and docs/CONTRACT.md; numbers are demo values. Actions mutate this world and the fetchers read it back.

import { TICKERS, VAULT_META, type Ticker } from "@/constants/vaults";
import type { Mode, PendingExit, Position, ProtocolStats, SharePricePoint, VaultRecord } from "@/lib/types";
import { filled, zeroed } from "@/lib/zeroed";

const DAY_MS = 86_400_000;
const BORROW_BPS = 590;
const SUPPLY_BPS = 480;
const ROUNDTRIP_BPS = 60;
const HOLD_HOURS = 720;
const ENTER_MARGIN = 200;
const EXIT_MARGIN = 100;
const CARRY_GUARD = 50;
const COST_APY_BPS = Math.round((ROUNDTRIP_BPS * 8760) / HOLD_HOURS);

function rng(seed: number) {
  let s = seed;
  return () => {
    s = (s * 9301 + 49297) % 233280;
    return s / 233280;
  };
}

interface Seed {
  price: number;
  tvlUsd: number;
  capUsd: number;
  fundingAvgBps: number;
  ageDays: number;
  mode: Mode;
}

const SEEDS: Record<Ticker, Seed> = {
  MSTR: { price: 330, tvlUsd: 840_000, capUsd: 1_500_000, fundingAvgBps: 4_100, ageDays: 90, mode: "funding" },
  SPY: { price: 650, tvlUsd: 1_120_000, capUsd: 3_000_000, fundingAvgBps: 900, ageDays: 90, mode: "idle" },
  GOOGL: { price: 250, tvlUsd: 710_000, capUsd: 2_000_000, fundingAvgBps: 2_600, ageDays: 90, mode: "funding" },
  QQQ: { price: 570, tvlUsd: 660_000, capUsd: 2_000_000, fundingAvgBps: 1_200, ageDays: 90, mode: "idle" },
  TSLA: { price: 412, tvlUsd: 2_180_000, capUsd: 4_000_000, fundingAvgBps: 3_500, ageDays: 90, mode: "funding" },
  NVDA: { price: 181, tvlUsd: 520_000, capUsd: 2_000_000, fundingAvgBps: 2_400, ageDays: 90, mode: "funding" },
  CRCL: { price: 128, tvlUsd: 310_000, capUsd: 800_000, fundingAvgBps: 3_400, ageDays: 60, mode: "funding" },
  HOOD: { price: 118, tvlUsd: 380_000, capUsd: 800_000, fundingAvgBps: 2_300, ageDays: 60, mode: "funding" },
  AAPL: { price: 238, tvlUsd: 90_000, capUsd: 200_000, fundingAvgBps: 700, ageDays: 4, mode: "idle" },
};

/** Hurdle per docs/DECISIONS.md D2, bps on basis notional. */
export function hurdleFor(mode: Mode, ltvBps: number, borrowBps: number, supplyBps: number): number {
  const lr = Math.round((ltvBps * borrowBps) / 10_000);
  const parkedOk = supplyBps >= borrowBps + CARRY_GUARD;
  // In Basis the comparison is against where the loan would go next: Parked if carry is positive, else Idle.
  const fromParked = mode === "parked" || (mode === "funding" && parkedOk);
  return (fromParked ? supplyBps : borrowBps) + lr + COST_APY_BPS;
}

function history(t: Ticker, seed: Seed, now: number): SharePricePoint[] {
  const r = rng(t.charCodeAt(0) * 7 + t.length * 13);
  const days = seed.ageDays;
  const ltv = VAULT_META[t].ltvBps / 10_000;
  const out: SharePricePoint[] = [];
  let usdc = 0;
  let mode: Mode = seed.mode;
  let left = 0;
  for (let d = 0; d <= days; d++) {
    if (d === 0) usdc = seed.ageDays < 7 ? -0.004 * seed.price * ltv * 0.04 : 0;
    if (left <= 0) {
      mode = mode === "funding" ? "idle" : "funding";
      left = mode === "funding" ? Math.floor(r() * 25) + 10 : Math.floor(r() * 8) + 3;
    }
    left--;
    if (d === days) mode = seed.mode;
    // Daily USDC per share on stock value: L·f − L(1+L)·r in funding, 0 in idle (loan repaid).
    const f = mode === "funding" ? (seed.fundingAvgBps / 10_000) * (0.8 + r() * 0.4) : 0;
    const daily = mode === "funding" ? (ltv * f - ltv * (1 + ltv) * (BORROW_BPS / 10_000)) * 0.85 : 0;
    usdc += (seed.price * daily) / 365;
    out.push({ date: new Date(now - (days - d) * DAY_MS).toISOString().slice(0, 10), usdcPerShare: usdc, mode });
  }
  // Force the tail to the seeded mode so the chart band matches the badge.
  for (let i = Math.max(0, out.length - 4); i < out.length; i++) out[i].mode = seed.mode;
  return out;
}

/** Growth of the share price (1 + usdc/p) in bps over a window, as the indexer's view would report it. */
function growthFromHistory(hist: SharePricePoint[], price: number) {
  const g = (days: number) => {
    if (hist.length < 2) return 0;
    const i = Math.max(0, hist.length - 1 - days);
    if (days > hist.length - 1) return 0;
    const a = 1 + hist[i].usdcPerShare / price;
    const b = 1 + hist[hist.length - 1].usdcPerShare / price;
    return Math.round(((b - a) / a) * 10_000);
  };
  const all = hist.length - 1;
  return { d7Bps: g(7), d30Bps: g(30), inceptionBps: g(all), inceptionDays: all };
}

function vault(t: Ticker, now: number): VaultRecord {
  const seed = SEEDS[t];
  const r = rng(t.charCodeAt(0) * 3 + 11);
  const hist = history(t, seed, now);
  const usdcPerShare = hist[hist.length - 1].usdcPerShare;
  const totalShares = seed.tvlUsd / seed.price;
  return {
    mode: seed.mode,
    marketOpen: true,
    priceUsd: seed.price,
    tvlUsd: seed.tvlUsd,
    capUsd: seed.capUsd,
    totalShares,
    fundingAvgBps: seed.fundingAvgBps,
    hurdleBps: hurdleFor(seed.mode, VAULT_META[t].ltvBps, BORROW_BPS, SUPPLY_BPS),
    vaultState: seed.mode === "funding" ? 3 : seed.mode === "parked" ? 1 : 0,
    ltvBps: VAULT_META[t].ltvBps,
    borrowApyBps: BORROW_BPS,
    supplyApyBps: SUPPLY_BPS,
    enterMarginBps: ENTER_MARGIN,
    exitMarginBps: EXIT_MARGIN,
    funding24h: Array.from({ length: 24 }, () => (seed.fundingAvgBps / 100) * (0.6 + r() * 0.8)),
    ageDays: seed.ageDays,
    usdcPerShare,
    sharePriceHistory: hist,
    trailing: growthFromHistory(hist, seed.price),
  };
}

export interface World {
  vaults: Record<Ticker, VaultRecord>;
  protocol: ProtocolStats;
  balances: Record<Ticker, number>;
  positions: Record<Ticker, Position>;
  pendingExits: Record<Ticker, PendingExit>;
  createdAt: number;
}

export const DEMO_WALLET = "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU";
export const DEMO_SETTLE_MS = 8_000;

function build(now: number): World {
  const vaults = {} as Record<Ticker, VaultRecord>;
  for (const t of TICKERS) vaults[t] = vault(t, now);
  const tvl = TICKERS.reduce((a, t) => a + vaults[t].tvlUsd, 0);
  const positions = filled(TICKERS, (): Position => ({ shares: 0, stockAmount: 0, usdcEarned: 0 }));
  positions.TSLA = { shares: 10, stockAmount: 10, usdcEarned: 10 * vaults.TSLA.usdcPerShare };
  positions.CRCL = { shares: 25, stockAmount: 25, usdcEarned: 25 * vaults.CRCL.usdcPerShare };
  positions.AAPL = { shares: 5, stockAmount: 5, usdcEarned: 5 * vaults.AAPL.usdcPerShare };
  const balances = zeroed(TICKERS);
  Object.assign(balances, { MSTR: 4, SPY: 3.2, GOOGL: 8, QQQ: 2, TSLA: 24.5, NVDA: 12.4, CRCL: 40, HOOD: 15, AAPL: 6 });
  return {
    vaults,
    protocol: {
      tvlUsd: tvl,
      avgYieldBps: 0, // filled by protocolStats() from history
      vaultsInFunding: TICKERS.filter((t) => vaults[t].mode === "funding").length,
      usdcPaid24h: 1_062,
      depositors: 1_284,
      borrowApyBps: BORROW_BPS,
      supplyApyBps: SUPPLY_BPS,
    },
    balances,
    positions,
    pendingExits: filled(TICKERS, (): PendingExit => ({ shares: 0, stockAmount: 0, usdcAmount: 0, readyAt: 0, ready: false, nonce: 0 })),
    createdAt: now,
  };
}

let world: World | undefined;

export function getWorld(): World {
  if (!world) world = build(Date.now());
  return world;
}

export function resetWorld() {
  world = undefined;
}

/** Advance the demo clock: accrue a little USDC on open positions and mark settled exits ready. */
export function tick(w: World, now = Date.now()) {
  for (const t of TICKERS) {
    const v = w.vaults[t];
    const p = w.positions[t];
    if (p.shares > 0 && v.mode === "funding") {
      // Sped-up demo accrual so the number visibly moves.
      const ltv = VAULT_META[t].ltvBps / 10_000;
      const perSec = (v.priceUsd * (ltv * (v.fundingAvgBps / 10_000)) * 0.85) / 31_536_000;
      const gain = perSec * 4_000;
      v.usdcPerShare += gain;
      p.usdcEarned = p.shares * v.usdcPerShare;
    }
    const e = w.pendingExits[t];
    if (e.shares > 0 && !e.ready && now >= e.readyAt) e.ready = true;
  }
}
