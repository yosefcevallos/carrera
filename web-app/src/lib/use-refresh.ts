"use client";

import { useCallback } from "react";
import type { Ticker } from "@/constants/vaults";
import { fetchPositions, type FetchPositionsOptions } from "./fetch-positions";
import { fetchVaults, type FetchVaultsOptions } from "./fetch-vaults";
import type { PositionsSnapshot, VaultExit, VaultsSnapshot } from "./types";
import { usePositionStore } from "@/store/position-provider";
import { useVaultStore } from "@/store/vault-provider";
import { useWalletStore } from "@/store/wallet-provider";

/** The setters the syncers commit through; the refresh path uses the very same ones. */
export interface RefreshSetters {
  setProtocol: (p: VaultsSnapshot["protocol"]) => void;
  setVaults: (v: VaultsSnapshot["vaults"]) => void;
  setBalances: (b: PositionsSnapshot["balances"]) => void;
  setPositions: (p: PositionsSnapshot["positions"]) => void;
  setExits: (e: PositionsSnapshot["exits"]) => void;
  setRaw: (r: { balancesRaw: PositionsSnapshot["balancesRaw"]; sharesRaw: PositionsSnapshot["sharesRaw"]; decimals: PositionsSnapshot["decimals"] }) => void;
}

export interface RefreshDeps {
  fetchVaults: (opts?: FetchVaultsOptions) => Promise<VaultsSnapshot>;
  fetchPositions: (address: string, opts?: FetchPositionsOptions) => Promise<PositionsSnapshot>;
}

const defaultDeps: RefreshDeps = { fetchVaults, fetchPositions };

/**
 * Refresh the moment a transaction resolves, success or failure: wallet balances, positions and
 * exit requests plus the vault records, straight from RPC, committed through the syncers' setters.
 * The Supabase history merge is skipped here so the refresh never waits on the indexer; the next
 * poll fills history back in. Never throws; failures are logged and the last good values stay.
 */
export async function refreshAfterAction(
  address: string,
  setters: RefreshSetters,
  knownExits?: Record<Ticker, VaultExit[]>,
  deps: RefreshDeps = defaultDeps,
): Promise<void> {
  const [vaults, positions] = await Promise.allSettled([
    deps.fetchVaults({ history: false }),
    address ? deps.fetchPositions(address, { indexer: false, knownExits }) : Promise.reject(new Error("no wallet")),
  ]);
  if (vaults.status === "fulfilled") {
    setters.setProtocol(vaults.value.protocol);
    setters.setVaults(vaults.value.vaults);
  } else console.error("[refresh] vaults failed:", vaults.reason);
  if (positions.status === "fulfilled") {
    const snap = positions.value;
    setters.setBalances(snap.balances);
    setters.setPositions(snap.positions);
    setters.setExits(snap.exits);
    setters.setRaw({ balancesRaw: snap.balancesRaw, sharesRaw: snap.sharesRaw, decimals: snap.decimals });
  } else if (address) console.error("[refresh] positions failed:", positions.reason);
}

/** Hook form of `refreshAfterAction` bound to the stores. Returns a function that resolves when the stores are updated. */
export function useRefresh() {
  const address = useWalletStore((s) => s.address);
  const setProtocol = useVaultStore((s) => s.setProtocol);
  const setVaults = useVaultStore((s) => s.setVaults);
  const setBalances = usePositionStore((s) => s.setBalances);
  const setPositions = usePositionStore((s) => s.setPositions);
  const setExits = usePositionStore((s) => s.setExits);
  const setRaw = usePositionStore((s) => s.setRaw);
  const exits = usePositionStore((s) => s.exits);

  return useCallback(
    () => refreshAfterAction(address, { setProtocol, setVaults, setBalances, setPositions, setExits, setRaw }, exits),
    [address, setProtocol, setVaults, setBalances, setPositions, setExits, setRaw, exits],
  );
}
