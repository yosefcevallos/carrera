// Polls the keeper's status endpoint and records ops snapshots, alerts and heartbeats.
import type { SupabaseClient } from "@supabase/supabase-js";

export interface KeeperStatus {
  keeper: {
    instance_id: string;
    is_leader: boolean;
    last_hourly_run_ts: number;
    last_fast_run_ts: number;
    sol_balance: number;
    alerts: { level: "warn" | "crit"; vault: string | null; message: string; ts: number }[];
  };
  vaults: {
    symbol: string;
    state: string;
    step: number;
    ltv_bps: number;
    margin_bps: number | null;
    net_delta: { qty: number; usd_e6: number };
    carry: { accrued_usdc_e6: number; ann_net_bps: number; estimated: boolean };
    legs: unknown[];
    nav_usd_e6: number;
    share_price_stock_e6: number;
    nav_slot: number;
  }[];
}

const tsIso = (unix: number) => (unix > 0 ? new Date(unix * 1000).toISOString() : null);

export function snapshotRows(status: KeeperStatus, now: Date) {
  const ts = now.toISOString();
  return status.vaults.map((v) => ({
    vault_symbol: v.symbol,
    ts,
    state: v.state,
    step: v.step,
    ltv_bps: v.ltv_bps,
    margin_bps: v.margin_bps,
    net_delta_qty: v.net_delta.qty,
    carry_accrued_usdc_e6: v.carry.accrued_usdc_e6,
    carry_ann_net_bps: v.carry.ann_net_bps,
    legs: v.legs,
  }));
}

export async function recordKeeperStatus(db: SupabaseClient, status: KeeperStatus, now = new Date()): Promise<void> {
  const snaps = snapshotRows(status, now);
  if (snaps.length > 0) {
    const { error } = await db.from("keeper_snapshots").upsert(snaps, { onConflict: "vault_symbol,ts" });
    if (error) throw new Error(`keeper_snapshots: ${error.message}`);
  }

  const hb = {
    instance_id: status.keeper.instance_id,
    is_leader: status.keeper.is_leader,
    last_hourly_ts: tsIso(status.keeper.last_hourly_run_ts),
    last_fast_ts: tsIso(status.keeper.last_fast_run_ts),
    sol_balance: status.keeper.sol_balance,
    updated_at: now.toISOString(),
  };
  const { error: hbErr } = await db.from("keeper_heartbeats").upsert(hb, { onConflict: "instance_id" });
  if (hbErr) throw new Error(`keeper_heartbeats: ${hbErr.message}`);

  // Alerts: open ones not reported any more get resolved; new ones get inserted once.
  const { data: open, error: openErr } = await db
    .from("keeper_alerts").select("id, vault_symbol, message").is("resolved_at", null);
  if (openErr) throw new Error(`keeper_alerts read: ${openErr.message}`);
  const key = (v: string | null, m: string) => `${v ?? ""}|${m}`;
  const reported = new Map(status.keeper.alerts.map((a) => [key(a.vault, a.message), a]));
  const openKeys = new Set((open ?? []).map((o) => key(o.vault_symbol as string | null, o.message as string)));

  const toResolve = (open ?? []).filter((o) => !reported.has(key(o.vault_symbol as string | null, o.message as string))).map((o) => o.id as number);
  if (toResolve.length > 0) {
    const { error } = await db.from("keeper_alerts").update({ resolved_at: now.toISOString() }).in("id", toResolve);
    if (error) throw new Error(`keeper_alerts resolve: ${error.message}`);
  }
  const toInsert = [...reported.values()].filter((a) => !openKeys.has(key(a.vault, a.message))).map((a) => ({
    ts: tsIso(a.ts) ?? now.toISOString(),
    level: a.level,
    vault_symbol: a.vault,
    message: a.message,
  }));
  if (toInsert.length > 0) {
    const { error } = await db.from("keeper_alerts").insert(toInsert);
    if (error) throw new Error(`keeper_alerts insert: ${error.message}`);
  }
}

export function startKeeperPoller(db: SupabaseClient, keeperUrl: string, intervalMs: number): NodeJS.Timeout {
  const tick = async () => {
    try {
      const res = await fetch(`${keeperUrl.replace(/\/$/, "")}/status`);
      if (!res.ok) throw new Error(`status ${res.status}`);
      await recordKeeperStatus(db, (await res.json()) as KeeperStatus);
    } catch (err) {
      console.error("[keeper] poll failed:", err);
    }
  };
  void tick();
  return setInterval(tick, intervalMs);
}
