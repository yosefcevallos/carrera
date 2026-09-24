import { TICKERS, type Ticker } from "@/constants/vaults";
import { DATA_SOURCE, KEEPER_URL } from "./chain/config";
import { mockFetchHistory, mockFetchOps } from "./mock/ops";
import { type Health, type HistoryPoint, type OpsSnapshot, type StatusResponse, zeroBook, zeroKeeper } from "./ops-types";
import { filled } from "./zeroed";

/** A book for every ticker and a keeper block, whatever the keeper returned. */
export async function fetchOps(): Promise<OpsSnapshot> {
  if (DATA_SOURCE !== "rpc") return mockFetchOps();
  const res = await fetch(`${KEEPER_URL}/status`, { cache: "no-store" });
  if (!res.ok) throw new Error(`keeper /status ${res.status}`);
  const json = (await res.json()) as Partial<StatusResponse>;
  const books = filled(TICKERS, () => zeroBook());
  for (const b of json.vaults ?? []) {
    if ((TICKERS as readonly string[]).includes(b.symbol)) books[b.symbol as Ticker] = b;
  }
  return { keeper: { ...zeroKeeper(), ...(json.keeper ?? {}) }, books };
}

export async function fetchOpsHistory(ticker: Ticker, hours = 168): Promise<HistoryPoint[]> {
  if (DATA_SOURCE !== "rpc") return mockFetchHistory(ticker, hours);
  const res = await fetch(`${KEEPER_URL}/history?vault=${ticker}&hours=${hours}`, { cache: "no-store" });
  if (!res.ok) throw new Error(`keeper /history ${res.status}`);
  return (await res.json()) as HistoryPoint[];
}

export async function fetchOpsHealth(): Promise<Health> {
  if (DATA_SOURCE !== "rpc") return "ok";
  try {
    const res = await fetch(`${KEEPER_URL}/healthz`, { cache: "no-store" });
    return res.ok ? "ok" : "stale";
  } catch {
    return "stale";
  }
}
