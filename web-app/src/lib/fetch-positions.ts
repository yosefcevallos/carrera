import type { Ticker } from "@/constants/vaults";
import { DATA_SOURCE } from "./chain/config";
import { mockFetchPositions } from "./mock";
import type { PositionsSnapshot, VaultExit } from "./types";

export interface FetchPositionsOptions {
  /** Read exit rows from the indexer (Supabase). Default true; the post-action refresh passes false. */
  indexer?: boolean;
  /** Exit requests already in the store, re-read on chain when the indexer is skipped. */
  knownExits?: Record<Ticker, VaultExit[]>;
}

/** Returns balances, positions and exit requests for every ticker. */
export async function fetchPositions(address: string, opts: FetchPositionsOptions = {}): Promise<PositionsSnapshot> {
  if (DATA_SOURCE === "rpc") {
    const { rpcFetchPositions } = await import("./chain/rpc");
    return rpcFetchPositions(address, opts);
  }
  return mockFetchPositions();
}
