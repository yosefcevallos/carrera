// RPC data source: reads Registry and the nine OverlayVault accounts and maps them to the UI shapes.
// Current values come from chain. History (sharePriceHistory, trailing growth, the 24h waveform,
// 24h payouts, depositors, pending exits) comes from the indexer through Supabase when
// NEXT_PUBLIC_SUPABASE_* is set, and stays zero otherwise.
import { Connection, PublicKey } from "@solana/web3.js";
import { TICKERS, VAULT_META, type Ticker } from "@/constants/vaults";
import type { FundingSample, Mode, PositionsSnapshot, VaultRecord, VaultsSnapshot } from "@/lib/types";
import { filled, zeroed } from "@/lib/zeroed";
import { RPC_URL, STOCK_TOKEN_PROGRAM_ID } from "./config";
import { fetchExitRows, fetchHistory, mapExits } from "@/lib/history";
import { decodeExitRequest, decodeOverlayVault, decodeRegistry, VaultState, type OverlayVaultAccount } from "./layout";
import { XSTOCK_MINTS } from "./mints";
import { ata, pda } from "./pda";

let conn: Connection | undefined;
export function connection(): Connection {
  if (!conn) conn = new Connection(RPC_URL, "confirmed");
  return conn;
}

const e6 = (n: bigint) => Number(n) / 1_000_000;
const DEFAULT_DECIMALS = 8;

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
    vaultState: 0,
    ltvBps: 0,
    borrowApyBps: 0,
    supplyApyBps: 0,
    enterMarginBps: 0,
    exitMarginBps: 0,
    fundingSamples: [],
    ageDays: 0,
    usdcPerShare: 0,
    sharePriceHistory: [],
    trailing: { d7Bps: 0, d30Bps: 0, inceptionBps: 0, inceptionDays: 0 },
  };
}

export function toRecord(v: OverlayVaultAccount, decimals: number, supplyApyBps = 0, registryBorrowBps = 0): VaultRecord {
  const price = e6(v.priceE6);
  const scale = 10 ** decimals;
  const shares = Number(v.totalShares) / scale;
  const depositorQty = Number(v.collateralQty - v.basisSpotQty) / scale;
  // Ring buffer, oldest first, in the program's scaled unit. The ring has no timestamps, so they
  // are derived backwards from the last sample time, one hour apart.
  const n = v.fundingSamples;
  const lastTs = Number(v.lastFundingTs) * 1000;
  const fundingSamples: FundingSample[] = [];
  for (let i = 0; i < n; i++) {
    const idx = (v.fundingHead - n + i + 48) % 24;
    fundingSamples.push({ ts: lastTs - (n - 1 - i) * 3_600_000, rateScaled: Number(v.funding[idx]) });
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
    vaultState: v.state,
    ltvBps: v.params.ltv_bps,
    borrowApyBps: v.lastRule.rBps || registryBorrowBps,
    supplyApyBps,
    enterMarginBps: v.params.enter_margin_bps,
    exitMarginBps: v.params.exit_margin_bps,
    fundingSamples,
    ageDays: 0,
    usdcPerShare,
    sharePriceHistory: [],
    trailing: { d7Bps: 0, d30Bps: 0, inceptionBps: 0, inceptionDays: 0 },
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
    const decoded = decodeOverlayVault(info.data);
    const rec = toRecord(decoded, decoded.stockDecimals || 8, reg?.supplyApyBps ?? 0, reg?.borrowApyBps ?? 0);
    vaults[t] = rec;
    tvl += rec.tvlUsd;
    if (rec.mode === "funding") inFunding++;
  });
  const prices = zeroed(TICKERS);
  for (const t of TICKERS) prices[t] = vaults[t].priceUsd;
  const history = await fetchHistory(prices).catch((err) => {
    console.error("[rpc] history fetch failed, keeping zeros:", err);
    return undefined;
  });
  let avgWeighted = 0;
  if (history) {
    for (const t of TICKERS) {
      const h = history.vaults[t];
      vaults[t] = { ...vaults[t], sharePriceHistory: h.sharePriceHistory, fundingSamples: h.fundingSamples.length ? h.fundingSamples : vaults[t].fundingSamples, trailing: h.trailing, ageDays: h.ageDays };
      if (h.trailing.d30Bps) avgWeighted += vaults[t].tvlUsd * (h.trailing.d30Bps * (365 / 30));
    }
  }
  return {
    protocol: {
      tvlUsd: tvl,
      avgYieldBps: tvl > 0 ? Math.round(avgWeighted / tvl) : 0,
      vaultsInFunding: inFunding,
      usdcPaid24h: history?.usdcPaid24h ?? 0,
      depositors: history?.depositors ?? 0,
      borrowApyBps: reg?.borrowApyBps ?? 0,
      supplyApyBps: reg?.supplyApyBps ?? 0,
    },
    vaults,
  };
}

export async function rpcFetchPositions(address: string): Promise<PositionsSnapshot> {
  const c = connection();
  const owner = new PublicKey(address);
  const stockAtas = TICKERS.map((t) => ata(owner, XSTOCK_MINTS[t], STOCK_TOKEN_PROGRAM_ID));
  const shareAtas = TICKERS.map((t) => ata(owner, pda.shareMint(pda.vault(XSTOCK_MINTS[t]))));
  const [stocks, shares] = await Promise.all([
    c.getMultipleParsedAccounts(stockAtas),
    c.getMultipleParsedAccounts(shareAtas),
  ]);
  const balances = zeroed(TICKERS);
  const positions = filled(TICKERS, () => ({ shares: 0, stockAmount: 0, usdcEarned: 0 }));
  // Pending exits: rows from the indexer, then the on-chain ExitRequest (when the row carries a
  // nonce) is authoritative for status.
  const rows = await fetchExitRows(address).catch((err) => {
    console.error("[rpc] exits fetch failed, keeping zeros:", err);
    return [];
  });
  const withNonce = rows.filter((r) => r.nonce != null && (TICKERS as readonly string[]).includes(r.vault_symbol));
  if (withNonce.length) {
    const keys = withNonce.map((r) => pda.exitRequest(pda.vault(XSTOCK_MINTS[r.vault_symbol as Ticker]), owner, BigInt(r.nonce as number)));
    const accts = await c.getMultipleAccountsInfo(keys);
    accts.forEach((a, i) => {
      if (!a) return;
      const onChain = decodeExitRequest(a.data);
      withNonce[i].status = onChain.status;
      withNonce[i].shares = onChain.shares.toString();
    });
  }
  const pendingExits = mapExits(rows, 8);
  // Raw base units only. xStocks carry Token-2022 ScaledUiAmount, so `uiAmount` is a scaled
  // display figure that does not round-trip to what the wallet actually holds.
  const rawOf = (acc: (typeof stocks.value)[number]): { raw: bigint; decimals: number } => {
    const d = acc?.data;
    if (!d || !("parsed" in d)) return { raw: 0n, decimals: DEFAULT_DECIMALS };
    const ta = d.parsed?.info?.tokenAmount;
    return { raw: BigInt(ta?.amount ?? "0"), decimals: Number(ta?.decimals ?? DEFAULT_DECIMALS) };
  };
  const balancesRaw = filled(TICKERS, () => "0");
  const sharesRaw = filled(TICKERS, () => "0");
  const decimals = filled(TICKERS, () => DEFAULT_DECIMALS);
  TICKERS.forEach((t, i) => {
    const b = rawOf(stocks.value[i]);
    const sh = rawOf(shares.value[i]);
    const dec = stocks.value[i] ? b.decimals : shares.value[i] ? sh.decimals : DEFAULT_DECIMALS;
    decimals[t] = dec;
    balancesRaw[t] = b.raw.toString();
    sharesRaw[t] = sh.raw.toString();
    balances[t] = Number(b.raw) / 10 ** dec;
    const s = Number(sh.raw) / 10 ** dec;
    positions[t] = { shares: s, stockAmount: s, usdcEarned: 0 };
  });
  return { balances, positions, pendingExits, balancesRaw, sharesRaw, decimals };
}
