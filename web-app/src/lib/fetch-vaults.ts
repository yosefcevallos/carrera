import { DATA_SOURCE } from "./chain/config";
import { mockFetchVaults } from "./mock";
import type { VaultsSnapshot } from "./types";

export interface FetchVaultsOptions {
  /** Merge indexer history (Supabase). Default true; the post-action refresh passes false. */
  history?: boolean;
}

/** Returns a value for every vault key and every protocol stat. */
export async function fetchVaults(opts: FetchVaultsOptions = {}): Promise<VaultsSnapshot> {
  if (DATA_SOURCE === "rpc") {
    const { rpcFetchVaults } = await import("./chain/rpc");
    return rpcFetchVaults(opts);
  }
  return mockFetchVaults();
}
