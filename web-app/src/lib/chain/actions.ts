// User actions. In mock mode they mutate the demo world; in rpc mode they build, send and confirm
// real transactions. The `build*Tx` functions are shared by the browser path (wallet adapter) and
// `scripts/devnet-flow.ts` (keypair), so both exercise the same instructions.
import { ComputeBudgetProgram, Connection, PublicKey, Transaction, type TransactionInstruction } from "@solana/web3.js";
import { VAULT_META, type Ticker } from "@/constants/vaults";
import { DATA_SOURCE, STOCK_TOKEN_PROGRAM_ID, TOKEN_PROGRAM_ID, USDC_MINT } from "./config";
import { mockCancelExit, mockDeposit, mockRedeem, mockRequestExit } from "@/lib/mock";
import { rememberLocalExit } from "@/lib/local-exits";

export interface Signer {
  publicKey: PublicKey;
  /** Signs and sends. The transaction already carries feePayer and recentBlockhash. */
  sendTransaction: (tx: Transaction) => Promise<string>;
}

/** Micro-lamports per compute unit added to every user transaction. */
export const PRIORITY_FEE_MICROLAMPORTS = 50_000;

export const DEFAULT_STOCK_DECIMALS = 8;

export function toBase(amount: number, decimals: number): bigint {
  return BigInt(Math.round(amount * 10 ** decimals));
}

/** Decimals of the vault's stock mint, read from the OverlayVault account; falls back to 8. */
export async function stockDecimals(conn: Connection, ticker: Ticker): Promise<number> {
  try {
    const [{ decodeOverlayVault }, { XSTOCK_MINTS }, { pda }] = await Promise.all([import("./layout"), import("./mints"), import("./pda")]);
    const info = await conn.getAccountInfo(pda.vault(XSTOCK_MINTS[ticker]));
    if (!info) return DEFAULT_STOCK_DECIMALS;
    const d = decodeOverlayVault(info.data).stockDecimals;
    return d > 0 ? d : DEFAULT_STOCK_DECIMALS;
  } catch {
    return DEFAULT_STOCK_DECIMALS;
  }
}

export interface BuiltTx {
  tx: Transaction;
  blockhash: string;
  lastValidBlockHeight: number;
}

async function finalize(conn: Connection, feePayer: PublicKey, ixs: TransactionInstruction[]): Promise<BuiltTx> {
  const { blockhash, lastValidBlockHeight } = await conn.getLatestBlockhash("confirmed");
  const tx = new Transaction({ feePayer, blockhash, lastValidBlockHeight });
  tx.add(ComputeBudgetProgram.setComputeUnitPrice({ microLamports: PRIORITY_FEE_MICROLAMPORTS }), ...ixs);
  return { tx, blockhash, lastValidBlockHeight };
}

/** deposit(qty_raw) with the user's share ATA created idempotently first. `qtyRaw` is in base units. */
export async function buildDepositTx(conn: Connection, user: PublicKey, ticker: Ticker, qtyRaw: bigint): Promise<BuiltTx> {
  const [{ depositIx, vaultKeys, createAtaIdempotentIx }, { XSTOCK_MINTS }] = await Promise.all([import("./ix"), import("./mints")]);
  const k = vaultKeys(XSTOCK_MINTS[ticker]);
  return finalize(conn, user, [
    createAtaIdempotentIx(user, user, k.shareMint, TOKEN_PROGRAM_ID),
    depositIx(user, k, qtyRaw, 0n),
  ]);
}

/** request_exit(shares_raw, nonce) into the vault's current open epoch. `sharesRaw` is in base units. */
export async function buildRequestExitTx(conn: Connection, user: PublicKey, ticker: Ticker, sharesRaw: bigint): Promise<BuiltTx & { nonce: bigint; epochId: bigint }> {
  const [{ requestExitIx, vaultKeys }, { XSTOCK_MINTS }, { decodeOverlayVault }] = await Promise.all([import("./ix"), import("./mints"), import("./layout")]);
  const k = vaultKeys(XSTOCK_MINTS[ticker]);
  const info = await conn.getAccountInfo(k.vault);
  if (!info) throw new Error(`Vault ${ticker} not found on this cluster.`);
  const v = decodeOverlayVault(info.data);
  const nonce = BigInt(Date.now());
  const built = await finalize(conn, user, [requestExitIx(user, k, sharesRaw, nonce, v.epochId)]);
  return { ...built, nonce, epochId: v.epochId };
}

/** redeem for an existing ExitRequest (nonce); creates the stock and USDC ATAs idempotently first. */
export async function buildRedeemTx(conn: Connection, user: PublicKey, ticker: Ticker, nonce: bigint): Promise<BuiltTx> {
  const [{ redeemIx, vaultKeys, createAtaIdempotentIx }, { XSTOCK_MINTS }, { decodeExitRequest }, { pda }] = await Promise.all([
    import("./ix"), import("./mints"), import("./layout"), import("./pda"),
  ]);
  const k = vaultKeys(XSTOCK_MINTS[ticker]);
  const info = await conn.getAccountInfo(pda.exitRequest(k.vault, user, nonce));
  if (!info) throw new Error("No withdrawal request found for this wallet.");
  const req = decodeExitRequest(info.data);
  return finalize(conn, user, [
    createAtaIdempotentIx(user, user, k.xstockMint, STOCK_TOKEN_PROGRAM_ID),
    createAtaIdempotentIx(user, user, USDC_MINT, TOKEN_PROGRAM_ID),
    redeemIx(user, k, USDC_MINT, nonce, req.epochId),
  ]);
}

/**
 * Send through the signer and wait for confirmation by polling `getSignatureStatuses`.
 * Polling rather than `confirmTransaction` because the browser talks to the HTTP-only
 * /api/rpc proxy, which has no websocket for the subscription-based path.
 */
export async function sendAndConfirm(conn: Connection, signer: Signer, built: BuiltTx, timeoutMs = 60_000): Promise<string> {
  const sig = await signer.sendTransaction(built.tx);
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    const { value } = await conn.getSignatureStatuses([sig]);
    const st = value[0];
    if (st?.err) throw new Error(`Transaction ${sig} failed: ${JSON.stringify(st.err)}`);
    if (st && (st.confirmationStatus === "confirmed" || st.confirmationStatus === "finalized")) return sig;
    const height = await conn.getBlockHeight("confirmed").catch(() => 0);
    if (height > built.lastValidBlockHeight) throw new Error(`Transaction ${sig} expired before it confirmed.`);
    await new Promise((r) => setTimeout(r, 1500));
  }
  throw new Error(`Transaction ${sig} was sent but not confirmed within ${Math.round(timeoutMs / 1000)}s. Check it in an explorer.`);
}

const ANCHOR_ERRORS: Record<string, string> = {
  NavStale: "Vault price is refreshing, try again in a minute.",
  DepositCapExceeded: "This vault is at its cap.",
  Paused: "Deposits are paused right now.",
  MarketClosed: "The market is closed, try during NYSE hours.",
  EpochNotSettled: "Your withdrawal hasn't settled yet.",
  EpochNotClosed: "This epoch is still open.",
  AccountNotInitialized: "A token account was missing. Please try again.",
  WrongState: "The vault is changing state, try again in a minute.",
};

/** Map an RPC / Anchor failure to a sentence for the form's error line; the full error is logged. */
export function explainError(e: unknown, token: string): string {
  console.error("[actions]", e);
  const msg = e instanceof Error ? e.message : String(e);
  if (/User rejected|rejected the request|user rejected/i.test(msg)) return "You cancelled the transaction in your wallet.";
  const code = msg.match(/Error Code: (\w+)/)?.[1] ?? msg.match(/"(\w+)"\s*:\s*\{?\s*"Custom"/)?.[1];
  if (code && ANCHOR_ERRORS[code]) return ANCHOR_ERRORS[code];
  if (/insufficient funds|insufficient lamports|0x1\b/.test(msg)) return /lamports/.test(msg) ? "Not enough SOL for fees." : `Not enough ${token} in your wallet.`;
  if (/custom program error: 0x1\b/.test(msg)) return `Not enough ${token} in your wallet.`;
  if (code) return code.replace(/([a-z])([A-Z])/g, "$1 $2");
  if (/blockhash|expired/i.test(msg)) return "The transaction expired before it confirmed. Try again.";
  return msg.length > 160 ? msg.slice(0, 157) + "…" : msg;
}

async function rpcConnection(): Promise<Connection> {
  const { connection } = await import("./rpc");
  return connection();
}

/** `amount.raw` is what is transacted; `amount.ui` is only used by the mock world. */
export interface Amount {
  raw: bigint;
  ui: number;
}

export async function deposit(ticker: Ticker, amount: Amount, signer?: Signer): Promise<void> {
  if (DATA_SOURCE === "mock") return mockDeposit(ticker, amount.ui);
  if (!signer) throw new Error("Connect a wallet first.");
  const conn = await rpcConnection();
  try {
    await sendAndConfirm(conn, signer, await buildDepositTx(conn, signer.publicKey, ticker, amount.raw));
  } catch (e) {
    throw new Error(explainError(e, VAULT_META[ticker].token));
  }
}

/**
 * `shares.raw` is the exact share quantity to escrow; `shares.ui` is only used by the mock world
 * (as stock amount). Resolves with the request's nonce, already remembered in localStorage so the
 * Requests tab can show it before the indexer does.
 */
export async function requestExit(ticker: Ticker, shares: Amount, signer?: Signer): Promise<string> {
  if (DATA_SOURCE === "mock") return mockRequestExit(ticker, shares.ui);
  if (!signer) throw new Error("Connect a wallet first.");
  const conn = await rpcConnection();
  try {
    const built = await buildRequestExitTx(conn, signer.publicKey, ticker, shares.raw);
    await sendAndConfirm(conn, signer, built);
    rememberLocalExit(signer.publicKey.toBase58(), ticker, { nonce: built.nonce.toString(), requestedAt: Date.now(), sharesRaw: shares.raw.toString() });
    return built.nonce.toString();
  } catch (e) {
    throw new Error(explainError(e, VAULT_META[ticker].token));
  }
}

/** cancel_exit for an open request in the still-open epoch. */
export async function cancelExit(ticker: Ticker, nonce: string, signer?: Signer): Promise<void> {
  if (DATA_SOURCE === "mock") return mockCancelExit(ticker, nonce);
  if (!signer) throw new Error("Connect a wallet first.");
  const conn = await rpcConnection();
  try {
    const [{ cancelExitIx, vaultKeys }, { XSTOCK_MINTS }, { decodeExitRequest }, { pda }] = await Promise.all([
      import("./ix"), import("./mints"), import("./layout"), import("./pda"),
    ]);
    const k = vaultKeys(XSTOCK_MINTS[ticker]);
    const info = await conn.getAccountInfo(pda.exitRequest(k.vault, signer.publicKey, BigInt(nonce)));
    if (!info) throw new Error("No withdrawal request found for this wallet.");
    const req = decodeExitRequest(info.data);
    await sendAndConfirm(conn, signer, await finalize(conn, signer.publicKey, [cancelExitIx(signer.publicKey, k, BigInt(nonce), req.epochId)]));
  } catch (e) {
    throw new Error(explainError(e, VAULT_META[ticker].token));
  }
}

export async function redeem(ticker: Ticker, nonce: string, signer?: Signer): Promise<{ stock: number; usdc: number }> {
  if (DATA_SOURCE === "mock") return mockRedeem(ticker, nonce);
  if (!signer) throw new Error("Connect a wallet first.");
  const conn = await rpcConnection();
  try {
    await sendAndConfirm(conn, signer, await buildRedeemTx(conn, signer.publicKey, ticker, BigInt(nonce)));
  } catch (e) {
    throw new Error(explainError(e, VAULT_META[ticker].token));
  }
  return { stock: 0, usdc: 0 };
}
