// RPC data source: reads Registry and the nine OverlayVault accounts and maps them to the UI shapes.
// Current values come from chain. History (sharePriceHistory, trailing growth, the 24h waveform,
// 24h payouts, depositors, pending exits) comes from the indexer through Supabase when
// NEXT_PUBLIC_SUPABASE_* is set, and stays zero otherwise.
import { Connection, PublicKey } from "@solana/web3.js";
import { TICKERS, VAULT_META, type Ticker } from "@/constants/vaults";
import type { ExitStatus, FundingSample, Mode, PositionsSnapshot, VaultRecord, VaultsSnapshot } from "@/lib/types";
import type { FetchVaultsOptions } from "@/lib/fetch-vaults";
import type { FetchPositionsOptions } from "@/lib/fetch-positions";
import { filled, zeroed } from "@/lib/zeroed";
import { RPC_URL, STOCK_TOKEN_PROGRAM_ID } from "./config";
import { fetchExitRows, fetchHistory, mapExits, type ExitRow } from "@/lib/history";
import { forgetLocalExits, readLocalExits } from "@/lib/local-exits";
import { decodeExitEpoch, decodeExitRequest, decodeOverlayVault, decodeRegistry, VaultState, type OverlayVaultAccount } from "./layout";
import { resolveExitStatus } from "@/lib/exits";
import { XSTOCK_MINTS } from "./mints";
import { ata, pda } from "./pda";

let conn: Connection | undefined;
export function connection(): Connection {
  if (!conn) conn = new Connection(RPC_URL, "confirmed");
  return conn;
}

const e6 = (n: bigint) => Number(n) / 1_000_000;
const DEFAULT_DECIMALS = 8;
const EXIT_STATUS_CODE: Record<ExitStatus, number> = { open: 0, settled: 1, redeemed: 2, cancelled: 3 };

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

export async function rpcFetchVaults(opts: FetchVaultsOptions = {}): Promise<VaultsSnapshot> {
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
  // The post-action refresh skips the indexer so it never waits on Supabase; the next poll merges it.
  const history = opts.history === false
    ? undefined
    : await fetchHistory(prices).catch((err) => {
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

export async function rpcFetchPositions(address: string, opts: FetchPositionsOptions = {}): Promise<PositionsSnapshot> {
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
  // Exit requests: indexer rows plus any this browser sent (localStorage), then the on-chain
  // ExitRequest for every nonce is authoritative for status and shares.
  // Post-action refresh: reuse the rows already in the store instead of asking the indexer.
  const rows: ExitRow[] = opts.indexer === false
    ? TICKERS.flatMap((t) => (opts.knownExits?.[t] ?? []).map((e): ExitRow => ({
        vault_symbol: t, nonce: e.nonce, shares: Math.round(e.shares * 1e8), epoch_id: e.epochId, status: EXIT_STATUS_CODE[e.status],
        requested_at: new Date(e.requestedAt).toISOString(), stock_out: e.stockAmount !== e.shares ? Math.round(e.stockAmount * 1e8) : null, usdc_out: e.usdcAmount ? Math.round(e.usdcAmount * 1e6) : null,
      })))
    : await fetchExitRows(address).catch((err) => {
        console.error("[rpc] exits fetch failed, keeping zeros:", err);
        return [] as ExitRow[];
      });
  const local = readLocalExits(address);
  const known = new Set(rows.map((r) => `${r.vault_symbol}:${r.nonce}`));
  for (const t of TICKERS) {
    for (const le of local[t] ?? []) {
      if (known.has(`${t}:${le.nonce}`)) continue;
      rows.push({ vault_symbol: t, nonce: le.nonce, shares: le.sharesRaw, epoch_id: 0, status: 0, requested_at: new Date(le.requestedAt).toISOString(), stock_out: null, usdc_out: null });
    }
  }
  const withNonce = rows.filter((r) => r.nonce != null && (TICKERS as readonly string[]).includes(r.vault_symbol));
  const gone = filled(TICKERS, () => [] as string[]);
  if (withNonce.length) {
    const keys = withNonce.map((r) => pda.exitRequest(pda.vault(XSTOCK_MINTS[r.vault_symbol as Ticker]), owner, BigInt(String(r.nonce))));
    const accts = await c.getMultipleAccountsInfo(keys);
    const chain = new Map<number, { status: number; shares: bigint; epochId: bigint }>();
    accts.forEach((a, i) => {
      const r = withNonce[i];
      if (!a) {
        // No account and no indexer row: a local nonce that never landed, or was closed. Drop it.
        if (!known.has(`${r.vault_symbol}:${r.nonce}`)) gone[r.vault_symbol as Ticker].push(String(r.nonce));
        return;
      }
      const onChain = decodeExitRequest(a.data);
      chain.set(i, { status: onChain.status, shares: onChain.shares, epochId: onChain.epochId });
      r.shares = onChain.shares.toString();
      r.epoch_id = onChain.epochId.toString();
    });
    // Settlement lives on the ExitEpoch, one read per distinct (vault, epoch).
    const epochKeys = new Map<string, PublicKey>();
    withNonce.forEach((r) => epochKeys.set(`${r.vault_symbol}:${r.epoch_id}`, pda.exitEpoch(pda.vault(XSTOCK_MINTS[r.vault_symbol as Ticker]), BigInt(String(r.epoch_id)))));
    const epochList = [...epochKeys.entries()];
    const epochAccts = epochList.length ? await c.getMultipleAccountsInfo(epochList.map(([, k]) => k)) : [];
    const epochs = new Map<string, ReturnType<typeof decodeExitEpoch>>();
    epochAccts.forEach((a, i) => {
      if (a) epochs.set(epochList[i][0], decodeExitEpoch(a.data));
    });
    withNonce.forEach((r, i) => {
      const ep = epochs.get(`${r.vault_symbol}:${r.epoch_id}`);
      const status = resolveExitStatus(chain.get(i)?.status, ep?.settled, r.status);
      r.status = ["open", "settled", "redeemed", "cancelled"].indexOf(status);
      if (status === "settled" && ep) {
        // Exact payout from the epoch's per-share values; the indexer only knows it after redeem.
        const shares = BigInt(String(r.shares));
        r.stock_out = ((shares * ep.stockPerShareE6) / 1_000_000n).toString();
        r.usdc_out = ((shares * ep.usdcPerShareE6) / 1_000_000n).toString();
      }
    });
  }
  for (const t of TICKERS) forgetLocalExits(address, t, gone[t]);
  const exits = mapExits(rows.filter((r) => !gone[r.vault_symbol as Ticker]?.includes(String(r.nonce))), 8);
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
  return { balances, positions, exits, balancesRaw, sharesRaw, decimals };
}
