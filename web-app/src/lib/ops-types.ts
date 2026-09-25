// Wire types for the keeper's status server (keeper/src/status.rs). Field names are the
// keeper's snake_case so the fetcher can pass JSON through untouched. `null` on a wire
// field means "not applicable" (a leg with no liquidation price), never "not loaded".
import type { Ticker } from "@/constants/vaults";

export type BookState = "idle" | "parked" | "winding" | "basis" | "unwinding" | "sizingup" | "partialunwinding";
export type LegKind = "long_spot" | "borrow_usdc" | "supply_usdc" | "short_perp";
export type AlertLevel = "warn" | "crit";
export type Decision = "none" | "to_basis" | "to_parked" | "to_idle";

export interface Leg {
  kind: LegKind;
  venue: string;
  /** Base units: 10^8 for stock legs, 10^6 for USDC legs */
  size: number;
  mark_e6: number;
  /** Annualised bps: 0 spot, −borrow, +supply, +funding */
  rate_bps: number;
  liq_price_e6: number | null;
  liq_distance_bps: number | null;
}

export interface NetDelta {
  qty: number;
  usd_e6: number;
}

export interface Carry {
  accrued_usdc_e6: number;
  ann_net_bps: number;
  estimated: boolean;
}

export interface RuleView {
  f_avg_bps: number;
  parked_apy_bps: number;
  r_bps: number;
  hurdle_bps: number;
  enter_bps: number;
  exit_bps: number;
  decision: Decision;
  samples: number;
}

export interface VaultBook {
  symbol: string;
  state: BookState;
  step: number;
  market_open: boolean;
  /** Unix seconds when the keeper first saw the current state; 0 before the first read */
  opened_ts: number;
  tier: number;
  legs: Leg[];
  net_delta: NetDelta;
  carry: Carry;
  rule: RuleView;
  ltv_bps: number;
  liq_ltv_bps: number;
  /** Phoenix margin, only meaningful in Basis */
  margin_bps: number | null;
  min_margin_bps: number;
  emergency_ltv_bps: number;
  nav_usd_e6: number;
  share_price_stock_e6: number;
  nav_slot: number;
  nav_age_slots: number;
  pending_exit_shares: number;
  epoch_id: number;
}

export interface AlertRecord {
  level: AlertLevel;
  vault: string | null;
  message: string;
  ts: number;
}

export interface KeeperView {
  instance_id: string;
  is_leader: boolean;
  last_hourly_run_ts: number;
  last_fast_run_ts: number;
  hourly_ok: boolean;
  fast_ok: boolean;
  sol_balance: number;
  program_id: string;
  cluster: string;
  alerts: AlertRecord[];
}

export interface StatusResponse {
  keeper: KeeperView;
  vaults: VaultBook[];
}

export interface HistoryPoint {
  ts: number;
  state: BookState;
  f_avg_bps: number;
  hurdle_bps: number;
  ltv_bps: number;
  margin_bps: number | null;
  nav_usd_e6: number;
  share_price_stock_e6: number;
}

export type Health = "unknown" | "ok" | "stale";

/** What the fetcher returns: a book for every ticker plus the keeper block. */
export interface OpsSnapshot {
  keeper: KeeperView;
  books: Record<Ticker, VaultBook>;
}

export function zeroBook(symbol = ""): VaultBook {
  return {
    symbol,
    state: "idle",
    step: 0,
    market_open: false,
    opened_ts: 0,
    tier: 0,
    legs: [],
    net_delta: { qty: 0, usd_e6: 0 },
    carry: { accrued_usdc_e6: 0, ann_net_bps: 0, estimated: true },
    rule: { f_avg_bps: 0, parked_apy_bps: 0, r_bps: 0, hurdle_bps: 0, enter_bps: 0, exit_bps: 0, decision: "none", samples: 0 },
    ltv_bps: 0,
    liq_ltv_bps: 0,
    margin_bps: null,
    min_margin_bps: 0,
    emergency_ltv_bps: 0,
    nav_usd_e6: 0,
    share_price_stock_e6: 0,
    nav_slot: 0,
    nav_age_slots: 0,
    pending_exit_shares: 0,
    epoch_id: 0,
  };
}

export function zeroKeeper(): KeeperView {
  return {
    instance_id: "",
    is_leader: false,
    last_hourly_run_ts: 0,
    last_fast_run_ts: 0,
    hourly_ok: false,
    fast_ok: false,
    sol_balance: 0,
    program_id: "",
    cluster: "",
    alerts: [],
  };
}
