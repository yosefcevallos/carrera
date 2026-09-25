// Backfills and keeps up to date the hourly Phoenix funding history per vault, so the web
// app can show 7 days of bars. Rows go into funding_samples in the program's unit
// (hourly rate in bps × 1e6, positive when longs pay shorts), keyed on (vault, ts).
import type { SupabaseClient } from "@supabase/supabase-js";

export const FUNDING_SCALE = 1_000_000;

export interface PhoenixRatesResponse {
  symbol: string;
  rates: { timestamp: number; fundingRatePercentage: string }[];
}

export interface FundingRow {
  vault_symbol: string;
  ts: string;
  rate_hourly_scaled: number;
}

/** fundingRatePercentage is percent per hour; program unit is bps per hour × 1e6. */
export function toScaled(fundingRatePercentage: string): number {
  return Math.round(Number(fundingRatePercentage) * 100 * FUNDING_SCALE);
}

export function fundingRows(symbol: string, res: PhoenixRatesResponse): FundingRow[] {
  return res.rates.map((r) => ({
    vault_symbol: symbol,
    ts: new Date(r.timestamp * 1000).toISOString(),
    rate_hourly_scaled: toScaled(r.fundingRatePercentage),
  }));
}

export interface PhoenixOptions {
  db: SupabaseClient;
  symbols: string[];
  apiUrl: string;
  historyHours: number;
  intervalMs: number;
}

export async function fetchRates(apiUrl: string, symbol: string, limit: number): Promise<PhoenixRatesResponse> {
  const res = await fetch(`${apiUrl.replace(/\/$/, "")}/v1/funding/${symbol}/rates?limit=${limit}`);
  if (!res.ok) throw new Error(`phoenix ${symbol}: status ${res.status}`);
  return (await res.json()) as PhoenixRatesResponse;
}

export async function recordPhoenixFunding(o: Omit<PhoenixOptions, "intervalMs">): Promise<Record<string, number>> {
  const written: Record<string, number> = {};
  for (const symbol of o.symbols) {
    try {
      const rows = fundingRows(symbol, await fetchRates(o.apiUrl, symbol, o.historyHours));
      if (rows.length > 0) {
        const { error } = await o.db.from("funding_samples").upsert(rows, { onConflict: "vault_symbol,ts" });
        if (error) throw new Error(error.message);
      }
      written[symbol] = rows.length;
      if (rows.length < o.historyHours) console.warn(`[phoenix] ${symbol}: ${rows.length}/${o.historyHours} rates`);
    } catch (err) {
      console.error(`[phoenix] ${symbol} failed:`, err);
      written[symbol] = 0;
    }
  }
  return written;
}

export function startPhoenixFunding(o: PhoenixOptions): NodeJS.Timeout {
  const tick = async () => {
    const w = await recordPhoenixFunding(o);
    const total = Object.values(w).reduce((a, b) => a + b, 0);
    console.log(`[phoenix] wrote ${total} funding rows (${Object.entries(w).map(([s, n]) => `${s}:${n}`).join(" ")})`);
  };
  void tick();
  return setInterval(tick, o.intervalMs);
}
