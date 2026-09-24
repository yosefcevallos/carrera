// Turns decoded events into table rows and upserts them with the service role.
import { createClient, type SupabaseClient } from "@supabase/supabase-js";
import { payloadToJson, type DecodedEvent } from "./decode.js";

export interface TxContext {
  signature: string;
  slot: number;
  blockTime: Date;
}

export type VaultLookup = Map<string, string>; // vault pubkey -> symbol

const str = (v: unknown) => (typeof v === "bigint" ? v.toString() : String(v));
const num = (v: unknown) => Number(v);

export class Writer {
  constructor(
    readonly db: SupabaseClient,
    readonly vaults: VaultLookup,
  ) {}

  static async connect(url: string, serviceRoleKey: string): Promise<Writer> {
    const db = createClient(url, serviceRoleKey, { auth: { persistSession: false } });
    const { data, error } = await db.from("vaults").select("symbol, vault_pubkey");
    if (error) throw new Error(`load vaults: ${error.message}`);
    const lookup: VaultLookup = new Map();
    for (const row of data ?? []) lookup.set(row.vault_pubkey as string, row.symbol as string);
    return new Writer(db, lookup);
  }

  private symbolOf(ev: DecodedEvent): string | null {
    const v = ev.payload.vault;
    return typeof v === "string" ? (this.vaults.get(v) ?? null) : null;
  }

  private async upsert(table: string, rows: Record<string, unknown>[], onConflict: string, ignore = false) {
    if (rows.length === 0) return;
    const { error } = await this.db.from(table).upsert(rows, { onConflict, ignoreDuplicates: ignore });
    if (error) throw new Error(`${table}: ${error.message}`);
  }

  /** Writes the raw log and every derived row for one transaction. Idempotent. */
  async writeTransaction(ctx: TxContext, events: DecodedEvent[]): Promise<void> {
    if (events.length === 0) return;
    const ts = ctx.blockTime.toISOString();

    await this.upsert(
      "program_events",
      events.map((e) => ({
        slot: ctx.slot,
        signature: ctx.signature,
        ix_index: e.ixIndex,
        log_index: e.logIndex,
        event_name: e.name,
        vault_symbol: this.symbolOf(e),
        payload: payloadToJson(e.payload),
        block_time: ts,
      })),
      "signature,log_index",
      true,
    );

    // Latest StateChanged in the tx gives RuleEvaluated its resulting state.
    let lastState: number | null = null;

    for (const e of events) {
      const p = e.payload;
      const symbol = this.symbolOf(e);
      if (!symbol && e.name !== "KaminoRatesRecorded" && e.name !== "Paused" && e.name !== "Unpaused") continue;
      const base = { vault_symbol: symbol, ts, slot: ctx.slot, signature: ctx.signature, log_index: e.logIndex };

      switch (e.name) {
        case "NavRefreshed":
          await this.upsert(
            "nav_samples",
            [{ vault_symbol: symbol, ts, slot: ctx.slot, nav_usd_e6: str(p.nav_usd_e6), share_price_stock_e6: str(p.share_price_stock_e6), price_e6: str(p.price_e6) }],
            "vault_symbol,ts",
          );
          break;
        case "FundingRecorded":
          await this.upsert(
            "funding_samples",
            [{ vault_symbol: symbol, ts, rate_hourly_scaled: str(p.rate_bps_e6_hourly) }],
            "vault_symbol,ts",
          );
          break;
        case "StateChanged":
          lastState = num(p.to);
          await this.upsert(
            "state_changes",
            [{ ...base, from_state: num(p.from), to_state: num(p.to), step: num(p.step) }],
            "signature,log_index", true,
          );
          break;
        case "RuleEvaluated":
          await this.upsert(
            "rule_samples",
            [{
              vault_symbol: symbol, ts,
              f_avg_bps: str(p.f_avg_bps), parked_apy_bps: num(p.parked_apy_bps), r_bps: num(p.r_bps),
              hurdle_bps: str(p.hurdle_bps), decision: num(p.decision),
              state: lastState ?? (await this.currentState(symbol!)),
            }],
            "vault_symbol,ts",
          );
          break;
        case "Rebalanced":
          await this.upsert("rebalances", [{ ...base, kind: REBALANCE_KIND[num(p.kind)] ?? `kind_${num(p.kind)}`, amount: str(p.amount) }], "signature,log_index", true);
          break;
        case "Deposited":
          await this.upsert("deposits", [{ ...base, user_pubkey: str(p.user), qty: str(p.qty), shares: str(p.shares) }], "signature,log_index", true);
          break;
        case "FeeCrystallised":
          await this.upsert("fees", [{ ...base, shares_minted: str(p.shares), high_water_e6: str(p.high_water_e6) }], "signature,log_index", true);
          break;
        case "EpochClosed":
          await this.upsert(
            "epochs",
            [{ vault_symbol: symbol, epoch_id: str(p.epoch_id), closed_at: ts, shares_total: str(p.shares_total), stock_owed: str(p.stock_owed), usdc_owed: str(p.usdc_owed) }],
            "vault_symbol,epoch_id",
          );
          break;
        case "EpochSettled":
          await this.settleEpoch(symbol!, p.epoch_id as bigint, p.stock_paid as bigint, p.usdc_paid as bigint, ts);
          break;
        case "ExitRequested":
          await this.upsert(
            "exits",
            [{ vault_symbol: symbol, user_pubkey: str(p.user), nonce: str(p.nonce), request_signature: ctx.signature, shares: str(p.shares), epoch_id: str(p.epoch_id), status: 0, requested_at: ts }],
            "vault_symbol,user_pubkey,nonce",
            true,
          );
          break;
        case "ExitCancelled":
          await this.closeExit(symbol!, str(p.user), str(p.nonce), { status: 3, cancelled_at: ts });
          break;
        case "Redeemed":
          await this.closeExit(symbol!, str(p.user), str(p.nonce), { status: 2, redeemed_at: ts, stock_out: str(p.stock), usdc_out: str(p.usdc) });
          break;
        default:
          break; // KaminoRatesRecorded, Paused, Unpaused live only in program_events
      }
    }
  }

  private async currentState(symbol: string): Promise<number> {
    const { data } = await this.db
      .from("state_changes").select("to_state").eq("vault_symbol", symbol)
      .order("ts", { ascending: false }).limit(1).maybeSingle();
    return (data?.to_state as number | undefined) ?? 0;
  }

  private async settleEpoch(symbol: string, epochId: bigint, stockPaid: bigint, usdcPaid: bigint, ts: string) {
    const { data } = await this.db
      .from("epochs").select("shares_total").eq("vault_symbol", symbol).eq("epoch_id", epochId.toString()).maybeSingle();
    const sharesTotal = BigInt((data?.shares_total as string | number | undefined) ?? 0);
    const perShare = (paid: bigint) => (sharesTotal > 0n ? (paid * 1_000_000n) / sharesTotal : 0n);
    await this.upsert(
      "epochs",
      [{
        vault_symbol: symbol, epoch_id: epochId.toString(), settled_at: ts,
        stock_paid: stockPaid.toString(), usdc_paid: usdcPaid.toString(),
        stock_per_share_e6: perShare(stockPaid).toString(), usdc_per_share_e6: perShare(usdcPaid).toString(),
      }],
      "vault_symbol,epoch_id",
    );
    const { error } = await this.db
      .from("exits").update({ status: 1 }).eq("vault_symbol", symbol).eq("epoch_id", epochId.toString()).eq("status", 0);
    if (error) throw new Error(`exits settle: ${error.message}`);
  }

  private async closeExit(symbol: string, user: string, nonce: string, patch: Record<string, unknown>) {
    const { error } = await this.db
      .from("exits").update(patch)
      .eq("vault_symbol", symbol).eq("user_pubkey", user).eq("nonce", nonce);
    if (error) throw new Error(`exits close: ${error.message}`);
  }
}

export const REBALANCE_KIND: Record<number, string> = {
  0: "to_kamino",
  1: "to_phoenix",
  2: "from_parked",
  3: "size_up",
  4: "unwind_partial",
};
