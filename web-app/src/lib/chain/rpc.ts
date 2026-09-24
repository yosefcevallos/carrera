// RPC data source: reads Registry and the nine OverlayVault accounts and maps them to the UI shapes.
// History (sharePriceHistory, funding24h) comes from the indexer once it exists; until then they are
// filled from the on-chain ring buffer and the current share price only.
import { Connection, PublicKey } from "@solana/web3.js";
import { TICKERS, VAULT_META, type Ticker } from "@/constants/vaults";
import type { Mode, PositionsSnapshot, VaultRecord, VaultsSnapshot } from "@/lib/types";
import { filled, zeroed } from "@/lib/zeroed";
import { RPC_URL } from "./config";
import { decodeOverlayVault, decodeRegistry, VaultState, type OverlayVaultAccount } from "./layout";
import { XSTOCK_MINTS } from "./mints";
import { ata, pda } from "./pda";

let conn: Connection | undefined;
export function connection(): Connection {
  if (!conn) conn = new Connection(RPC_URL, "confirmed");
  return conn;
}

const e6 = (n: bigint) => Number(n) / 1_000_000;

function modeOf(state: number): Mode {
  if (state === VaultState.Basis || state === VaultState.Winding) return "funding";
  if (state === VaultState.Parked || state === VaultState.Unwinding) return "parked";
  return "idle";
}

function emptyVault(): VaultRecord {
  return {
    mode: "idle",
    marketOpen: false,
    priceUsd: 0,
    tvlUsd: 0,
    capUsd: 0,
    totalShares: 0,
    fundingAvgBps: 0,
    hurdleBps: 0,
    enterMarginBps: 0,
    exitMarginBps: 0,
    funding24h: [],
    ageDays: 0,
    usdcPerShare: 0,
    sharePriceHistory: [],
  };
}

function toRecord(v: OverlayVaultAccount, decimals: number): VaultRecord {
  const price = e6(v.priceE6);
  const scale = 10 ** decimals;
  const shares = Number(v.totalShares) / scale;
  const depositorQty = Number(v.collateralQty - v.basisSpotQty) / scale;
  // Ring buffer, oldest first, hourly bps → annualised percent.
  const n = v.fundingSamples;
  const funding24h: number[] = [];
  for (let i = 0; i < n; i++) {
    const idx = (v.fundingHead - n + i + 48) % 24;
    funding24h.push((Number(v.funding[idx]) * 8760) / 100);
  }
  const usdcPerShare = shares > 0 ? (e6(v.navUsdE6) - depositorQty * price) / shares : 0;
  return {
    mode: modeOf(v.state),
    marketOpen: v.marketOpen,
    priceUsd: price,
    tvlUsd: depositorQty * price,
    capUsd: (Number(v.params.deposit_cap_stock) / scale) * price,
    totalShares: shares,
    fundingAvgBps: Number(v.lastRule.fAvgBps),
    hurdleBps: Number(v.lastRule.hurdleBps),
    enterMarginBps: v.params.enter_margin_bps,
    exitMarginBps: v.params.exit_margin_bps,
    funding24h,
    ageDays: 0,
    usdcPerShare,
    sharePriceHistory: [],
  };
}

export async function rpcFetchVaults(): Promise<VaultsSnapshot> {
  const c = connection();
  const keys = [pda.registry(), ...TICKERS.map((t) => pda.vault(XSTOCK_MINTS[t]))];
  const infos = await c.getMultipleAccountsInfo(keys);
  const reg = infos[0] ? decodeRegistry(infos[0].data) : undefined;
  const vaults = filled(TICKERS, emptyVault);
  let tvl = 0;
  let inFunding = 0;
  TICKERS.forEach((t, i) => {
    const info = infos[i + 1];
    if (!info) return;
    const rec = toRecord(decodeOverlayVault(info.data), 6);
    vaults[t] = rec;
    tvl += rec.tvlUsd;
    if (rec.mode === "funding") inFunding++;
  });
  return {
    protocol: {
      tvlUsd: tvl,
      avgYieldBps: 0,
      vaultsInFunding: inFunding,
      usdcPaid24h: 0,
      depositors: 0,
      borrowApyBps: reg?.borrowApyBps ?? 0,
      supplyApyBps: reg?.supplyApyBps ?? 0,
    },
    vaults,
  };
}

export async function rpcFetchPositions(address: string): Promise<PositionsSnapshot> {
  const c = connection();
  const owner = new PublicKey(address);
  const stockAtas = TICKERS.map((t) => ata(owner, XSTOCK_MINTS[t]));
  const shareAtas = TICKERS.map((t) => ata(owner, pda.shareMint(pda.vault(XSTOCK_MINTS[t]))));
  const [stocks, shares] = await Promise.all([
    c.getMultipleParsedAccounts(stockAtas),
    c.getMultipleParsedAccounts(shareAtas),
  ]);
  const balances = zeroed(TICKERS);
  const positions = filled(TICKERS, () => ({ shares: 0, stockAmount: 0, usdcEarned: 0 }));
  const pendingExits = filled(TICKERS, () => ({ shares: 0, stockAmount: 0, usdcAmount: 0, readyAt: 0, ready: false, nonce: 0 }));
  const amount = (acc: (typeof stocks.value)[number]) => {
    const d = acc?.data;
    if (!d || !("parsed" in d)) return 0;
    return Number(d.parsed?.info?.tokenAmount?.uiAmount ?? 0);
  };
  TICKERS.forEach((t, i) => {
    balances[t] = amount(stocks.value[i]);
    const s = amount(shares.value[i]);
    positions[t] = { shares: s, stockAmount: s, usdcEarned: 0 };
  });
  // Exit requests need the indexer (ExitRequest PDAs are keyed by nonce); left zeroed until it exists.
  return { balances, positions, pendingExits };
}
