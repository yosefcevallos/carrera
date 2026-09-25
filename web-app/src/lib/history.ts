// Indexer-backed history for rpc mode. Pure mappers take rows and always return every ticker;
// the fetchers wrap them with the Supabase client and fall back to empty rows on any failure.
import { TICKERS, type Ticker } from "@/constants/vaults";
import type { ExitStatus, FundingSample, Mode, SharePricePoint, TrailingGrowth, VaultExit } from "./types";
import { readyAtFor } from "./exits";
import { filled } from "./zeroed";
import { getSupabase } from "./supabase";

const DAY_MS = 86_400_000;
const E6 = 1_000_000;
/** funding_samples.rate_hourly_scaled is hourly bps × 1e6 */
const FUNDING_WINDOW = 168;

export interface NavRow { vault_symbol: string; ts: string; share_price_stock_e6: number | string; price_e6: number | string | null }
export interface RuleRow { vault_symbol: string; ts: string; state: number }
export interface TrailingRow {
  vault_symbol: string; inception_at: string | null;
  growth_7d_bps: number | string | null; growth_30d_bps: number | string | null; growth_inception_bps: number | string | null;
}
export interface FundingRow { vault_symbol: string; ts: string; rate_hourly_scaled: number | string }
export interface ProtocolRow { usdc_paid_24h: number | string | null; depositors: number | null }
export interface ExitRow {
  vault_symbol: string; nonce: number | string | null; shares: number | string; epoch_id: number | string;
  status: number; requested_at: string; stock_out: number | string | null; usdc_out: number | string | null;
}

export interface VaultHistory {
  sharePriceHistory: SharePricePoint[];
  fundingSamples: FundingSample[];
  trailing: TrailingGrowth;
  ageDays: number;
}

export interface HistorySnapshot {
  vaults: Record<Ticker, VaultHistory>;
  usdcPaid24h: number;
  depositors: number;
}

export interface HistoryRows {
  nav: NavRow[];
  rules: RuleRow[];
  trailing: TrailingRow[];
  funding: FundingRow[];
  protocol: ProtocolRow[];
}

const num = (v: number | string | null | undefined) => (v == null ? 0 : Number(v));
const isTicker = (s: string): s is Ticker => (TICKERS as readonly string[]).includes(s);

function modeOfState(state: number): Mode {
  if (state === 3 || state === 2) return "funding";
  if (state === 1 || state === 4) return "parked";
  return "idle";
}

export function emptyHistory(): VaultHistory {
  return { sharePriceHistory: [], fundingSamples: [], trailing: { d7Bps: 0, d30Bps: 0, inceptionBps: 0, inceptionDays: 0 }, ageDays: 0 };
}

/**
 * Map raw rows to per-ticker history. `priceUsd` is the current on-chain price per ticker, used when a
 * nav sample carries no price. Every ticker is present in the result even with no rows.
 */
export function mapHistory(rows: HistoryRows, priceUsd: Record<Ticker, number>, now = Date.now()): HistorySnapshot {
  const vaults = filled(TICKERS, emptyHistory);

  // Daily downsample of nav samples: last sample per UTC day, oldest first.
  const byDay = filled(TICKERS, () => new Map<string, { usdc: number }>());
  for (const r of rows.nav) {
    if (!isTicker(r.vault_symbol)) continue;
    const day = r.ts.slice(0, 10);
    const price = r.price_e6 != null ? num(r.price_e6) / E6 : priceUsd[r.vault_symbol];
    const usdc = (num(r.share_price_stock_e6) / E6 - 1) * price;
    byDay[r.vault_symbol].set(day, { usdc }); // rows are ordered by ts asc, so the last write wins
  }
  const lastStateByDay = filled(TICKERS, () => new Map<string, Mode>());
  for (const r of rows.rules) {
    if (!isTicker(r.vault_symbol)) continue;
    lastStateByDay[r.vault_symbol].set(r.ts.slice(0, 10), modeOfState(r.state));
  }
  for (const t of TICKERS) {
    const days = [...byDay[t].keys()].sort();
    let mode: Mode = "idle";
    vaults[t].sharePriceHistory = days.map((date) => {
      mode = lastStateByDay[t].get(date) ?? mode;
      return { date, usdcPerShare: byDay[t].get(date)!.usdc, mode };
    });
  }

  for (const r of rows.trailing) {
    if (!isTicker(r.vault_symbol)) continue;
    const inceptionDays = r.inception_at ? Math.max(0, Math.floor((now - Date.parse(r.inception_at)) / DAY_MS)) : 0;
    vaults[r.vault_symbol].trailing = {
      d7Bps: num(r.growth_7d_bps),
      d30Bps: num(r.growth_30d_bps),
      inceptionBps: num(r.growth_inception_bps),
      inceptionDays,
    };
    vaults[r.vault_symbol].ageDays = inceptionDays;
  }

  // Funding views return newest first; the waveform wants oldest first, capped at 168 samples.
  const funding = filled(TICKERS, () => [] as FundingSample[]);
  for (const r of rows.funding) {
    if (!isTicker(r.vault_symbol)) continue;
    funding[r.vault_symbol].push({ ts: Date.parse(r.ts), rateScaled: num(r.rate_hourly_scaled) });
  }
  for (const t of TICKERS) vaults[t].fundingSamples = funding[t].sort((a, b) => a.ts - b.ts).slice(-FUNDING_WINDOW);

  const p = rows.protocol[0];
  return { vaults, usdcPaid24h: p ? num(p.usdc_paid_24h) / E6 : 0, depositors: p?.depositors ?? 0 };
}

const EXIT_STATUS: ExitStatus[] = ["open", "settled", "redeemed", "cancelled"];

/** Every exit request per ticker for one wallet, newest first. Every ticker present. */
export function mapExits(rows: ExitRow[], shareDecimals = 8, epochLenSecs = 3600): Record<Ticker, VaultExit[]> {
  const out = filled(TICKERS, (): VaultExit[] => []);
  const scale = 10 ** shareDecimals;
  for (const r of rows) {
    if (!isTicker(r.vault_symbol) || r.nonce == null) continue;
    const shares = num(r.shares) / scale;
    const status = EXIT_STATUS[r.status] ?? "open";
    const paid = status === "settled" || status === "redeemed";
    const requestedAt = Date.parse(r.requested_at);
    out[r.vault_symbol].push({
      nonce: String(r.nonce),
      shares,
      stockAmount: paid && r.stock_out != null ? num(r.stock_out) / scale : shares,
      usdcAmount: paid && r.usdc_out != null ? num(r.usdc_out) / E6 : 0,
      epochId: num(r.epoch_id),
      status,
      requestedAt,
      readyAt: readyAtFor(requestedAt, epochLenSecs),
    });
  }
  for (const t of TICKERS) out[t].sort((a, b) => b.requestedAt - a.requestedAt);
  return out;
}

export async function fetchHistory(priceUsd: Record<Ticker, number>): Promise<HistorySnapshot> {
  const sb = getSupabase();
  const rows: HistoryRows = { nav: [], rules: [], trailing: [], funding: [], protocol: [] };
  if (!sb) return mapHistory(rows, priceUsd);
  const since = new Date(Date.now() - 90 * DAY_MS).toISOString();
  const [nav, rules, trailing, funding, protocol] = await Promise.all([
    sb.from("nav_samples").select("vault_symbol,ts,share_price_stock_e6,price_e6").gte("ts", since).order("ts", { ascending: true }).limit(50_000),
    sb.from("rule_samples").select("vault_symbol,ts,state").gte("ts", since).order("ts", { ascending: true }).limit(50_000),
    sb.from("v_trailing_yield").select("vault_symbol,inception_at,growth_7d_bps,growth_30d_bps,growth_inception_bps"),
    fetchFundingRows(sb),
    sb.from("v_protocol_stats").select("usdc_paid_24h,depositors"),
  ]);
  for (const r of [nav, rules, trailing, funding, protocol]) if (r.error) throw new Error(`[supabase] ${r.error.message}`);
  rows.nav = (nav.data ?? []) as NavRow[];
  rows.rules = (rules.data ?? []) as RuleRow[];
  rows.trailing = (trailing.data ?? []) as TrailingRow[];
  rows.funding = (funding.data ?? []) as FundingRow[];
  rows.protocol = (protocol.data ?? []) as ProtocolRow[];
  return mapHistory(rows, priceUsd);
}

type FundingResult = { data: FundingRow[] | null; error: { message: string } | null };

/** 7-day funding: `v_funding_7d`, or `funding_samples` for the last 7 days while the view is missing. */
async function fetchFundingRows(sb: NonNullable<ReturnType<typeof getSupabase>>): Promise<FundingResult> {
  const view = await sb.from("v_funding_7d").select("vault_symbol,ts,rate_hourly_scaled");
  if (!view.error) return view as FundingResult;
  const since = new Date(Date.now() - 7 * DAY_MS).toISOString();
  const raw = await sb.from("funding_samples").select("vault_symbol,ts,rate_hourly_scaled").gte("ts", since).order("ts", { ascending: false }).limit(FUNDING_WINDOW * TICKERS.length);
  return raw as FundingResult;
}

export async function fetchExitRows(address: string): Promise<ExitRow[]> {
  const sb = getSupabase();
  if (!sb) return [];
  const { data, error } = await sb
    .from("exits")
    .select("vault_symbol,nonce,shares,epoch_id,status,requested_at,stock_out,usdc_out")
    .eq("user_pubkey", address)
    .order("requested_at", { ascending: false })
    .limit(200);
  if (error) throw new Error(`[supabase] ${error.message}`);
  return (data ?? []) as ExitRow[];
}
