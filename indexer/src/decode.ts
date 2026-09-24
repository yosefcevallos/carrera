// Decodes Anchor events from transaction log messages.
// Event bytes = sha256("event:<Name>")[0..8] ++ borsh(fields).
// Field orders match program/programs/carrera_overlay/src/events.rs and the IDL; see EVENTS.md.
import { createHash } from "node:crypto";
import { Reader } from "./borsh.js";

export const EVENT_NAMES = [
  "Deposited",
  "ExitRequested",
  "ExitCancelled",
  "EpochClosed",
  "EpochSettled",
  "Redeemed",
  "StateChanged",
  "RuleEvaluated",
  "NavRefreshed",
  "FundingRecorded",
  "KaminoRatesRecorded",
  "Rebalanced",
  "FeeCrystallised",
  "Paused",
  "Unpaused",
] as const;
export type EventName = (typeof EVENT_NAMES)[number];

export function eventDiscriminator(name: string): Buffer {
  return createHash("sha256").update(`event:${name}`).digest().subarray(0, 8);
}

const DISCRIMINATORS = new Map<string, EventName>(
  EVENT_NAMES.map((n) => [eventDiscriminator(n).toString("hex"), n]),
);

// bigint-bearing fields are kept as bigint; callers stringify for JSON.
export type EventPayload = Record<string, string | number | bigint | boolean>;

export interface DecodedEvent {
  name: EventName;
  ixIndex: number;
  logIndex: number;
  payload: EventPayload;
}

const FIELD_DECODERS: Record<EventName, (r: Reader) => EventPayload> = {
  Deposited: (r) => ({ vault: r.pubkey(), user: r.pubkey(), qty: r.u64(), shares: r.u64() }),
  ExitRequested: (r) => ({ vault: r.pubkey(), user: r.pubkey(), nonce: r.u64(), shares: r.u64(), epoch_id: r.u64() }),
  ExitCancelled: (r) => ({ vault: r.pubkey(), user: r.pubkey(), nonce: r.u64(), shares: r.u64() }),
  EpochClosed: (r) => ({
    vault: r.pubkey(),
    epoch_id: r.u64(),
    shares_total: r.u64(),
    stock_owed: r.u64(),
    usdc_owed: r.u64(),
  }),
  EpochSettled: (r) => ({ vault: r.pubkey(), epoch_id: r.u64(), stock_paid: r.u64(), usdc_paid: r.u64() }),
  Redeemed: (r) => ({ vault: r.pubkey(), user: r.pubkey(), nonce: r.u64(), shares: r.u64(), stock: r.u64(), usdc: r.u64() }),
  StateChanged: (r) => ({ vault: r.pubkey(), from: r.u8(), to: r.u8(), step: r.u8() }),
  RuleEvaluated: (r) => ({
    vault: r.pubkey(),
    f_avg_bps: r.i64(),
    parked_apy_bps: r.u32(),
    r_bps: r.u32(),
    hurdle_bps: r.i64(),
    decision: r.u8(),
  }),
  NavRefreshed: (r) => ({ vault: r.pubkey(), nav_usd_e6: r.u64(), share_price_stock_e6: r.u64(), price_e6: r.u64() }),
  FundingRecorded: (r) => ({
    vault: r.pubkey(),
    rate_bps_e6_hourly: r.i64(),
    f_avg_bps: r.i64(),
    samples: r.u8(),
  }),
  KaminoRatesRecorded: (r) => ({ borrow_apy_bps: r.u32(), supply_apy_bps: r.u32() }),
  Rebalanced: (r) => ({ vault: r.pubkey(), kind: r.u8(), amount: r.u64() }),
  FeeCrystallised: (r) => ({ vault: r.pubkey(), shares: r.u64(), high_water_e6: r.u64() }),
  Paused: () => ({}),
  Unpaused: () => ({}),
};

export function decodeEventBytes(data: Buffer): { name: EventName; payload: EventPayload } | null {
  if (data.length < 8) return null;
  const name = DISCRIMINATORS.get(data.subarray(0, 8).toString("hex"));
  if (!name) return null;
  const reader = new Reader(data.subarray(8));
  try {
    return { name, payload: FIELD_DECODERS[name](reader) };
  } catch {
    return null;
  }
}

/**
 * Walks a transaction's log messages, tracking the program invoke stack so only
 * `Program data:` lines emitted while our program is on the stack are decoded.
 * ixIndex counts depth-1 invocations of any program (top-level instruction index).
 */
export function decodeLogs(logs: readonly string[], programId: string): DecodedEvent[] {
  const events: DecodedEvent[] = [];
  const stack: string[] = [];
  let ixIndex = -1;
  let logIndex = 0;

  for (const line of logs) {
    const invoke = /^Program (\S+) invoke \[(\d+)\]$/.exec(line);
    if (invoke) {
      const depth = Number(invoke[2]);
      if (depth === 1) ixIndex += 1;
      stack.push(invoke[1]);
      continue;
    }
    if (/^Program \S+ (success|failed)/.test(line)) {
      stack.pop();
      continue;
    }
    if (!line.startsWith("Program data: ")) continue;
    if (stack[stack.length - 1] !== programId) continue;

    const decoded = decodeEventBytes(Buffer.from(line.slice("Program data: ".length), "base64"));
    if (!decoded) continue;
    events.push({ name: decoded.name, ixIndex: Math.max(ixIndex, 0), logIndex, payload: decoded.payload });
    logIndex += 1;
  }
  return events;
}

/** JSON-safe copy of a payload (bigint → decimal string). */
export function payloadToJson(p: EventPayload): Record<string, string | number | boolean> {
  const out: Record<string, string | number | boolean> = {};
  for (const [k, v] of Object.entries(p)) out[k] = typeof v === "bigint" ? v.toString() : v;
  return out;
}
