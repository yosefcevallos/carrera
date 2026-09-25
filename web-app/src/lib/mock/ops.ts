// Deterministic mock of the keeper's status server. Exercises every state the dashboard
// renders: Basis books, one Parked, Idle vaults, and a Winding vault stuck long enough to
// trip the keeper's "stuck" alert. Numbers follow the same rules the keeper uses
// (keeper/src/status.rs) so the layout matches what a live keeper will send.
import { TICKERS, VAULT_META, type Ticker } from "@/constants/vaults";
import { filled } from "@/lib/zeroed";
import { STOCK_DECIMALS } from "@/lib/ops-math";
import type { AlertRecord, BookState, HistoryPoint, KeeperView, Leg, OpsSnapshot, VaultBook } from "@/lib/ops-types";

const BORROW_BPS = 590;
const SUPPLY_BPS = 480;
const ROUNDTRIP_BPS = 60;
const HOLD_HOURS = 720;
const ENTER_MARGIN = 200;
const EXIT_MARGIN = 100;
const CARRY_GUARD = 50;
const COST_APY_BPS = Math.round((ROUNDTRIP_BPS * 8760) / HOLD_HOURS);
const TIER_INDEX = { A: 0, B: 1, C: 2, D: 3 } as const;
const LIQ_LTV = { A: 7000, B: 6500, C: 5000, D: 4000 } as const;
const MIN_MARGIN = { A: 1200, B: 1200, C: 1000, D: 800 } as const;
const PHOENIX_BUFFER = [2600, 2600, 2100, 1600];
const STOCK = 10 ** STOCK_DECIMALS;

interface Seed {
  price: number;
  tvlUsd: number;
  fundingAvgBps: number;
  state: BookState;
  step: number;
  /** Seconds the vault has been in `state` */
  ageSec: number;
  /** Margin excess over the tier minimum, bps; ignored outside Basis */
  marginExtra: number;
}

const H = 3600;
const D = 86_400;
const SEEDS: Record<Ticker, Seed> = {
  TSLA: { price: 412.3, tvlUsd: 2_180_000, fundingAvgBps: 3_500, state: "basis", step: 0, ageSec: 19 * D, marginExtra: 220 },
  GOOGL: { price: 250, tvlUsd: 710_000, fundingAvgBps: 2_600, state: "basis", step: 0, ageSec: 6 * D, marginExtra: 900 },
  CRCL: { price: 128, tvlUsd: 310_000, fundingAvgBps: 3_400, state: "basis", step: 0, ageSec: 2 * D, marginExtra: 700 },
  HOOD: { price: 118, tvlUsd: 380_000, fundingAvgBps: 2_300, state: "basis", step: 0, ageSec: 11 * D, marginExtra: 1_100 },
  MSTR: { price: 330, tvlUsd: 840_000, fundingAvgBps: 1_500, state: "parked", step: 0, ageSec: 3 * H, marginExtra: 0 },
  NVDA: { price: 181, tvlUsd: 520_000, fundingAvgBps: 2_400, state: "winding", step: 2, ageSec: 22 * 60, marginExtra: 0 },
  SPY: { price: 650, tvlUsd: 1_120_000, fundingAvgBps: 900, state: "idle", step: 0, ageSec: 5 * D, marginExtra: 0 },
  QQQ: { price: 570, tvlUsd: 660_000, fundingAvgBps: 1_200, state: "idle", step: 0, ageSec: 2 * D, marginExtra: 0 },
  AAPL: { price: 238, tvlUsd: 90_000, fundingAvgBps: 700, state: "idle", step: 0, ageSec: 4 * D, marginExtra: 0 },
};

function rng(seed: number) {
  let s = seed;
  return () => {
    s = (s * 9301 + 49297) % 233280;
    return s / 233280;
  };
}

/** NYSE regular session: weekdays 09:30–16:00 America/New_York. */
export function nyseOpen(date = new Date()): boolean {
  const parts = new Intl.DateTimeFormat("en-US", { timeZone: "America/New_York", weekday: "short", hour: "numeric", minute: "numeric", hour12: false }).formatToParts(date);
  const get = (t: string) => parts.find((p) => p.type === t)?.value ?? "";
  const wd = get("weekday");
  if (wd === "Sat" || wd === "Sun") return false;
  const mins = (parseInt(get("hour"), 10) % 24) * 60 + parseInt(get("minute"), 10);
  return mins >= 9 * 60 + 30 && mins < 16 * 60;
}

function hurdles(ltvBps: number) {
  const lr = Math.round((ltvBps * BORROW_BPS) / 10_000);
  return { fromParked: SUPPLY_BPS + lr + COST_APY_BPS, fromIdle: BORROW_BPS + lr + COST_APY_BPS };
}

function buildBook(t: Ticker, nowSec: number): VaultBook {
  const s = SEEDS[t];
  const meta = VAULT_META[t];
  const tier = TIER_INDEX[meta.tier];
  const L = meta.ltvBps / 10_000;
  const priceE6 = Math.round(s.price * 1e6);
  const V = s.tvlUsd;
  const Dusd = V * L;
  const collateralQty = Math.round((V / s.price) * STOCK);
  const inBasis = s.state === "basis" || (s.state === "winding" && s.step >= 1) || s.state === "unwinding";
  const basisSpot = inBasis ? Math.round((Dusd / s.price) * STOCK) : 0;
  const hasLoan = s.state !== "idle";
  const debt = hasLoan ? Math.round(Dusd * 1e6) : 0;
  const debtB = s.state === "basis" || (s.state === "winding" && s.step >= 2) ? Math.round(Dusd * L * 1e6) : 0;
  const parked = s.state === "parked" ? debt : 0;
  const short = s.state === "basis" || (s.state === "winding" && s.step >= 3) ? basisSpot : 0;
  const totalCollateral = collateralQty + basisSpot;
  const ltvBps = totalCollateral ? Math.round(((debt + debtB) / 1e6 / ((totalCollateral / STOCK) * s.price)) * 10_000) : 0;
  const liqLtv = LIQ_LTV[meta.tier];
  const kLiq = ltvBps ? { price: Math.round((priceE6 * ltvBps) / liqLtv), dist: 10_000 - Math.round((ltvBps * 10_000) / liqLtv) } : null;
  const fAvg = s.fundingAvgBps;

  const legs: Leg[] = [];
  if (basisSpot) legs.push({ kind: "long_spot", venue: "Kamino", size: basisSpot, mark_e6: priceE6, rate_bps: 0, liq_price_e6: kLiq?.price ?? null, liq_distance_bps: kLiq?.dist ?? null });
  if (debt + debtB) legs.push({ kind: "borrow_usdc", venue: "Kamino", size: debt + debtB, mark_e6: 1e6, rate_bps: -BORROW_BPS, liq_price_e6: kLiq?.price ?? null, liq_distance_bps: kLiq?.dist ?? null });
  if (parked) legs.push({ kind: "supply_usdc", venue: "Kamino", size: parked, mark_e6: 1e6, rate_bps: SUPPLY_BPS, liq_price_e6: null, liq_distance_bps: null });
  if (short) {
    const buf = PHOENIX_BUFFER[tier];
    legs.push({ kind: "short_perp", venue: "Phoenix", size: short, mark_e6: priceE6 + 165_000, rate_bps: fAvg, liq_price_e6: Math.round((priceE6 * (10_000 + buf)) / 10_000), liq_distance_bps: buf });
  }

  const h = hurdles(meta.ltvBps);
  const hurdle = s.state === "idle" ? h.fromIdle : h.fromParked;
  const carryOk = SUPPLY_BPS >= BORROW_BPS + CARRY_GUARD;
  let decision: VaultBook["rule"]["decision"] = "none";
  if ((s.state === "parked" || s.state === "idle") && fAvg > hurdle + ENTER_MARGIN) decision = "to_basis";
  else if (s.state === "basis" && fAvg < hurdle - EXIT_MARGIN) decision = carryOk ? "to_parked" : "to_idle";
  else if (s.state === "parked" && !carryOk) decision = "to_idle";

  const lr = Math.round((meta.ltvBps * BORROW_BPS) / 10_000);
  const gross = s.state === "basis" || s.state === "winding" || s.state === "unwinding" ? fAvg : s.state === "parked" ? SUPPLY_BPS : 0;
  const annNet = s.state === "idle" ? 0 : gross - BORROW_BPS - lr;
  const shortNotional = (short / STOCK) * s.price;
  const perYear = fAvg * shortNotional + SUPPLY_BPS * (parked / 1e6) - BORROW_BPS * ((debt + debtB) / 1e6);
  const accrued = Math.round(((perYear * s.ageSec) / (10_000 * 365 * D)) * 1e6);
  const netQty = basisSpot - short;

  const margin = s.state === "basis" ? MIN_MARGIN[meta.tier] + s.marginExtra : null;
  const opened = nowSec - s.ageSec;
  return {
    symbol: t,
    state: s.state,
    step: s.step,
    market_open: nyseOpen(new Date(nowSec * 1000)),
    opened_ts: opened,
    tier,
    legs,
    net_delta: { qty: netQty, usd_e6: Math.round((netQty / STOCK) * priceE6) },
    carry: { accrued_usdc_e6: accrued, ann_net_bps: annNet, estimated: true },
    rule: { f_avg_bps: fAvg, f_3h_bps: Math.round(fAvg * 1.1), parked_apy_bps: SUPPLY_BPS, r_bps: BORROW_BPS, be_bps: hurdle, hurdle_bps: hurdle, enter_bps: hurdle + ENTER_MARGIN, exit_bps: hurdle - EXIT_MARGIN, decision, samples: 24 },
    ltv_bps: ltvBps,
    liq_ltv_bps: liqLtv,
    margin_bps: margin,
    min_margin_bps: MIN_MARGIN[meta.tier],
    emergency_ltv_bps: liqLtv - 500,
    nav_usd_e6: Math.round(V * 1e6),
    share_price_stock_e6: 1_000_000 + Math.round(((accrued / 1e6) * 1e6) / Math.max(1, V)),
    nav_slot: 1_000_000,
    nav_age_slots: t === "AAPL" ? 420 : 12,
    pending_exit_shares: t === "TSLA" ? 10 * STOCK : 0,
    epoch_id: 1912,
  };
}

export interface OpsWorld {
  keeper: KeeperView;
  books: Record<Ticker, VaultBook>;
  history: Partial<Record<Ticker, HistoryPoint[]>>;
  createdSec: number;
}

let world: OpsWorld | undefined;

function build(nowSec: number): OpsWorld {
  const books = filled(TICKERS, () => buildBook("TSLA", nowSec));
  for (const t of TICKERS) books[t] = buildBook(t, nowSec);
  const alerts: AlertRecord[] = [
    { level: "crit", vault: "NVDA", message: "Winding for 22 min at step 2; the hourly pass has not resumed it", ts: nowSec - 7 * 60 },
    { level: "crit", vault: "TSLA", message: "Phoenix margin 14.2% is within 300 bps of the 12.0% minimum. Top up to restore.", ts: nowSec - 3 * 60 },
    { level: "warn", vault: "AAPL", message: "NAV cache is 420 slots old, past the 150-slot bound", ts: nowSec - 11 * 60 },
    { level: "warn", vault: "TSLA", message: "Funding fell from 41.0% to 35.0% in 8h. Net carry still 27.3% above borrow cost.", ts: nowSec - 40 * 60 },
  ];
  return {
    keeper: {
      instance_id: "keeper-a",
      is_leader: true,
      last_hourly_run_ts: nowSec - 23 * 60,
      last_fast_run_ts: nowSec - 40,
      hourly_ok: true,
      fast_ok: true,
      sol_balance: 1.84,
      program_id: "2GL5kpBSr1aAM6wHveCgeUD1AkzJ4qwMa5MVaSdC1ND7",
      cluster: "localnet (mock)",
      alerts,
    },
    books,
    history: {},
    createdSec: nowSec,
  };
}

export function getOpsWorld(nowSec = Math.floor(Date.now() / 1000)): OpsWorld {
  if (!world) world = build(nowSec);
  return world;
}

export function resetOpsWorld() {
  world = undefined;
}

const clone = <T>(v: T): T => structuredClone(v);

export async function mockFetchOps(): Promise<OpsSnapshot> {
  const w = getOpsWorld();
  const now = Math.floor(Date.now() / 1000);
  w.keeper.last_fast_run_ts = now - ((now - w.createdSec) % 60);
  for (const t of TICKERS) w.books[t].market_open = nyseOpen();
  return clone({ keeper: w.keeper, books: w.books });
}

/** Seven days at one sample per five minutes; the tail matches the book's current state. */
export async function mockFetchHistory(t: Ticker, hours = 168): Promise<HistoryPoint[]> {
  const w = getOpsWorld();
  if (!w.history[t]) {
    const b = w.books[t];
    const r = rng(t.charCodeAt(0) * 31 + 7);
    const stepSec = 300;
    const n = Math.floor((168 * 3600) / stepSec);
    const end = w.createdSec;
    const out: HistoryPoint[] = [];
    let f = b.rule.f_avg_bps * (0.7 + r() * 0.3);
    for (let i = 0; i <= n; i++) {
      const ts = end - (n - i) * stepSec;
      f += (r() - 0.5) * 60 + (b.rule.f_avg_bps - f) * 0.004;
      const inCurrent = ts >= b.opened_ts;
      let state: BookState = b.state;
      if (!inCurrent) state = f > b.rule.enter_bps ? "basis" : "idle";
      out.push({ ts, state, f_avg_bps: Math.round(f), hurdle_bps: b.rule.hurdle_bps, ltv_bps: b.ltv_bps, margin_bps: b.margin_bps, nav_usd_e6: b.nav_usd_e6, share_price_stock_e6: b.share_price_stock_e6 });
    }
    w.history[t] = out;
  }
  const since = w.createdSec - hours * 3600;
  return clone(w.history[t]!.filter((p) => p.ts >= since));
}

export type OpsActionKind = "rebalance_to_kamino" | "rebalance_to_phoenix" | "unwind_emergency" | "pause";

/** Simulates the crank's effect on the book and records what happened as an alert. */
export async function mockOpsAction(kind: OpsActionKind, t: Ticker): Promise<string> {
  const w = getOpsWorld();
  const b = w.books[t];
  const now = Math.floor(Date.now() / 1000);
  const note = (level: AlertRecord["level"], message: string) => w.keeper.alerts.push({ level, vault: kind === "pause" ? null : t, message, ts: now });
  switch (kind) {
    case "rebalance_to_kamino": {
      if (b.state !== "basis") throw new Error(`${t} is not in the basis trade, so there is no Phoenix collateral to move.`);
      b.ltv_bps = Math.max(0, b.ltv_bps - 300);
      if (b.margin_bps !== null) b.margin_bps = Math.max(b.min_margin_bps, b.margin_bps - 300);
      note("warn", "Rebalanced to Kamino: moved free Phoenix collateral to repay debt (ops)");
      return `Rebalanced ${t}: Phoenix free collateral repaid Kamino debt.`;
    }
    case "rebalance_to_phoenix": {
      if (b.state !== "basis") throw new Error(`${t} is not in the basis trade, so there is no Phoenix margin to top up.`);
      b.ltv_bps += 300;
      if (b.margin_bps !== null) b.margin_bps += 300;
      note("warn", "Rebalanced to Phoenix: borrowed on Kamino to top up perp margin (ops)");
      return `Topped up ${t} perp margin from Kamino.`;
    }
    case "unwind_emergency": {
      if (b.state !== "basis") throw new Error(`${t} is not in the basis trade.`);
      b.state = "unwinding";
      b.step = 0;
      b.opened_ts = now;
      b.carry = { accrued_usdc_e6: 0, ann_net_bps: b.carry.ann_net_bps, estimated: true };
      note("crit", "Emergency unwind started by ops");
      return `Emergency unwind started for ${t}. The keeper will run the steps.`;
    }
    case "pause": {
      note("crit", "Registry paused by ops. Deposits and mode changes stop; rebalances continue.");
      return "Registry paused.";
    }
  }
}
