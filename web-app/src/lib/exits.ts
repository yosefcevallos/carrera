import type { ExitStatus, VaultExit } from "./types";

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

/**
 * Status precedence for one request. The on-chain ExitRequest only moves to redeemed (2) or
 * cancelled (3) by the user; settlement is an epoch-level fact, so "settled" comes from the
 * on-chain ExitEpoch flag or the indexer row, never from the request account.
 */
export function resolveExitStatus(
  chainStatus: number | undefined,
  epochSettled: boolean | undefined,
  rowStatus: number | undefined,
  opts: {
    /** True when the ExitRequest account was read and is gone: the program closes it on redeem and cancel. */
    accountMissing?: boolean;
    /** A status already known locally; redeemed / cancelled never move backwards. */
    prior?: ExitStatus;
  } = {},
): ExitStatus {
  if (opts.prior === "redeemed" || opts.prior === "cancelled") return opts.prior;
  if (chainStatus === 2 || rowStatus === 2) return "redeemed";
  if (chainStatus === 3 || rowStatus === 3) return "cancelled";
  if (opts.accountMissing) return epochSettled || rowStatus === 1 ? "redeemed" : "cancelled";
  if (epochSettled || rowStatus === 1) return "settled";
  return "open";
}

/** Top of the next hour after `requestedAt`, the earliest an hourly epoch can settle it. */
export function readyAtFor(requestedAtMs: number, epochLenSecs = 3600): number {
  const len = epochLenSecs * 1000;
  return Math.ceil((requestedAtMs + 1) / len) * len;
}
