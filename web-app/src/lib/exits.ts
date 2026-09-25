import type { VaultExit } from "./types";

/** Number of requests still open or ready to claim; drives the "Requests · n" badge. */
export function activeExitCount(exits: VaultExit[]): number {
  return exits.filter((e) => e.status === "open" || e.status === "settled").length;
}

export function requestsTabLabel(exits: VaultExit[]): string {
  const n = activeExitCount(exits);
  return n > 0 ? `Requests · ${n}` : "Requests";
}

/** Sum of shares across requests that are not yet claimed or cancelled. */
export function pendingShares(exits: VaultExit[]): number {
  return exits.filter((e) => e.status === "open" || e.status === "settled").reduce((a, e) => a + e.shares, 0);
}

export const anyReady = (exits: VaultExit[]) => exits.some((e) => e.status === "settled");
export const anyOpen = (exits: VaultExit[]) => exits.some((e) => e.status === "open");

/** Top of the next hour after `requestedAt`, the earliest an hourly epoch can settle it. */
export function readyAtFor(requestedAtMs: number, epochLenSecs = 3600): number {
  const len = epochLenSecs * 1000;
  return Math.ceil((requestedAtMs + 1) / len) * len;
}
