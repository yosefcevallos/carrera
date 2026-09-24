import { DATA_SOURCE } from "./chain/config";
import { mockFetchPositions } from "./mock";
import type { PositionsSnapshot } from "./types";

/** Returns balances, positions and pending exits for every ticker. */
export async function fetchPositions(address: string): Promise<PositionsSnapshot> {
  if (DATA_SOURCE === "rpc") {
    const { rpcFetchPositions } = await import("./chain/rpc");
    return rpcFetchPositions(address);
  }
  return mockFetchPositions();
}
