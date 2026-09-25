import { TICKERS, VAULT_META, type Ticker } from "@/constants/vaults";
import type { PositionsSnapshot, VaultsSnapshot } from "@/lib/types";
import { trailingYield } from "@/lib/yield";
import { DEMO_SETTLE_MS, getWorld, tick } from "./world";

const clone = <T>(v: T): T => structuredClone(v);

export async function mockFetchVaults(): Promise<VaultsSnapshot> {
  const w = getWorld();
  tick(w);
  let weighted = 0;
  for (const t of TICKERS) {
    const v = w.vaults[t];
    weighted += trailingYield(v.sharePriceHistory, 30, v.priceUsd).apy * v.tvlUsd;
  }
  const protocol = { ...w.protocol, avgYieldBps: Math.round((weighted / w.protocol.tvlUsd) * 100) };
  return clone({ protocol, vaults: w.vaults });
}

export async function mockFetchPositions(): Promise<PositionsSnapshot> {
  const w = getWorld();
  tick(w);
  const raw = (n: number) => BigInt(Math.round(n * 1e8)).toString();
  const balancesRaw = {} as Record<Ticker, string>;
  const sharesRaw = {} as Record<Ticker, string>;
  const decimals = {} as Record<Ticker, number>;
  for (const t of TICKERS) {
    balancesRaw[t] = raw(w.balances[t]);
    sharesRaw[t] = raw(w.positions[t].shares);
    decimals[t] = 8;
  }
  return clone({ balances: w.balances, positions: w.positions, pendingExits: w.pendingExits, balancesRaw, sharesRaw, decimals });
}

export async function mockDeposit(ticker: Ticker, qty: number) {
  const w = getWorld();
  if (qty <= 0) throw new Error("Enter how much to deposit.");
  if (qty > w.balances[ticker]) throw new Error(`That's more than the ${w.balances[ticker]} ${VAULT_META[ticker].token} in your wallet.`);
  const v = w.vaults[ticker];
  w.balances[ticker] -= qty;
  const p = w.positions[ticker];
  // Shares minted at current NAV: one share = 1 stock + usdcPerShare, so qty stock buys qty·p/(p+usdc) shares.
  const shares = (qty * v.priceUsd) / (v.priceUsd + v.usdcPerShare);
  p.shares += shares;
  p.stockAmount += qty;
  p.usdcEarned = p.shares * v.usdcPerShare;
  v.tvlUsd += qty * v.priceUsd;
  v.totalShares += shares;
}

export async function mockRequestExit(ticker: Ticker, stockAmount: number) {
  const w = getWorld();
  const p = w.positions[ticker];
  if (stockAmount <= 0) throw new Error("Enter how much to withdraw.");
  if (stockAmount > p.stockAmount + 1e-9) throw new Error(`You have ${p.stockAmount} ${VAULT_META[ticker].token} in this vault.`);
  const frac = Math.min(1, stockAmount / p.stockAmount);
  const shares = p.shares * frac;
  const usdc = p.usdcEarned * frac;
  p.shares -= shares;
  p.stockAmount -= stockAmount;
  p.usdcEarned -= usdc;
  if (p.shares < 1e-9) Object.assign(p, { shares: 0, stockAmount: 0, usdcEarned: 0 });
  const e = w.pendingExits[ticker];
  e.shares += shares;
  e.stockAmount += stockAmount;
  e.usdcAmount += usdc;
  e.readyAt = Date.now() + DEMO_SETTLE_MS;
  e.ready = false;
  e.nonce += 1;
}

export async function mockRedeem(ticker: Ticker) {
  const w = getWorld();
  const e = w.pendingExits[ticker];
  if (e.shares <= 0 || !e.ready) throw new Error("Nothing ready to claim.");
  const v = w.vaults[ticker];
  // Spec §4.2: negative USDC reduces the stock leg.
  let stock = e.stockAmount;
  let usdc = e.usdcAmount;
  if (usdc < 0) {
    stock -= Math.abs(usdc) / v.priceUsd;
    usdc = 0;
  }
  w.balances[ticker] += stock;
  v.tvlUsd -= e.stockAmount * v.priceUsd;
  v.totalShares -= e.shares;
  Object.assign(e, { shares: 0, stockAmount: 0, usdcAmount: 0, readyAt: 0, ready: false });
  return { stock, usdc };
}
