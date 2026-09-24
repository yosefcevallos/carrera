// User actions. Mock mode simulates locally; rpc mode builds the instruction and asks the wallet to sign.
import { PublicKey, Transaction } from "@solana/web3.js";
import type { Ticker } from "@/constants/vaults";
import { DATA_SOURCE, USDC_MINT } from "./config";
import { mockDeposit, mockRedeem, mockRequestExit } from "@/lib/mock";

export interface Signer {
  publicKey: PublicKey;
  sendTransaction: (tx: Transaction) => Promise<string>;
}

const toBase = (amount: number, decimals = 6) => BigInt(Math.round(amount * 10 ** decimals));

export async function deposit(ticker: Ticker, qty: number, signer?: Signer): Promise<void> {
  if (DATA_SOURCE === "mock") return mockDeposit(ticker, qty);
  if (!signer) throw new Error("Connect a wallet first.");
  const [{ depositIx, vaultKeys }, { XSTOCK_MINTS }] = await Promise.all([import("./ix"), import("./mints")]);
  const ix = depositIx(signer.publicKey, vaultKeys(XSTOCK_MINTS[ticker]), toBase(qty), 0n);
  await signer.sendTransaction(new Transaction().add(ix));
}

export async function requestExit(ticker: Ticker, stockAmount: number, signer?: Signer, epochId = 0n): Promise<void> {
  if (DATA_SOURCE === "mock") return mockRequestExit(ticker, stockAmount);
  if (!signer) throw new Error("Connect a wallet first.");
  const [{ requestExitIx, vaultKeys }, { XSTOCK_MINTS }] = await Promise.all([import("./ix"), import("./mints")]);
  const nonce = BigInt(Date.now());
  const ix = requestExitIx(signer.publicKey, vaultKeys(XSTOCK_MINTS[ticker]), toBase(stockAmount), nonce, epochId);
  await signer.sendTransaction(new Transaction().add(ix));
}

export async function redeem(ticker: Ticker, signer?: Signer, nonce = 0n, epochId = 0n): Promise<{ stock: number; usdc: number }> {
  if (DATA_SOURCE === "mock") return mockRedeem(ticker);
  if (!signer) throw new Error("Connect a wallet first.");
  const [{ redeemIx, vaultKeys }, { XSTOCK_MINTS }] = await Promise.all([import("./ix"), import("./mints")]);
  const ix = redeemIx(signer.publicKey, vaultKeys(XSTOCK_MINTS[ticker]), USDC_MINT, nonce, epochId);
  await signer.sendTransaction(new Transaction().add(ix));
  return { stock: 0, usdc: 0 };
}
