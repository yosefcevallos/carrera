import { DATA_SOURCE } from "./chain/config";
import { mockFetchVaults } from "./mock";
import type { VaultsSnapshot } from "./types";

/** Returns a value for every vault key and every protocol stat. */
export async function fetchVaults(): Promise<VaultsSnapshot> {
  if (DATA_SOURCE === "rpc") {
    const { rpcFetchVaults } = await import("./chain/rpc");
    return rpcFetchVaults();
  }
  return mockFetchVaults();
}
