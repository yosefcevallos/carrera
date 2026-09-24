// Fetches a confirmed transaction's logs and block time by signature.
import { Connection } from "@solana/web3.js";
import type { TxContext } from "../write.js";

export interface FetchedTx extends TxContext {
  logs: string[];
}

export async function fetchTx(conn: Connection, signature: string): Promise<FetchedTx | null> {
  const tx = await conn.getTransaction(signature, { maxSupportedTransactionVersion: 0, commitment: "confirmed" });
  if (!tx || !tx.meta || tx.meta.err) return null;
  return {
    signature,
    slot: tx.slot,
    blockTime: new Date((tx.blockTime ?? Math.floor(Date.now() / 1000)) * 1000),
    logs: tx.meta.logMessages ?? [],
  };
}
