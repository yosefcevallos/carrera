"use client";

import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import type { Transaction } from "@solana/web3.js";
import type { Signer } from "./chain/actions";

/**
 * Wallet-adapter bridge for the action helpers. Undefined until a wallet is connected.
 * `useConnection` is the app's own endpoint (the /api/rpc proxy, see Providers), so the wallet
 * signs and sends through the same node the app reads from; the builders set feePayer and
 * blockhash beforehand and `sendAndConfirm` waits on that blockhash.
 */
export function useSigner(): Signer | undefined {
  const { connection } = useConnection();
  const { publicKey, sendTransaction } = useWallet();
  if (!publicKey) return undefined;
  return {
    publicKey,
    sendTransaction: (tx: Transaction) => sendTransaction(tx, connection, { skipPreflight: false, preflightCommitment: "confirmed" }),
  };
}
