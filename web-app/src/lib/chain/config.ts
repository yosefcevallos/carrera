import { PublicKey } from "@solana/web3.js";

export type DataSource = "mock" | "rpc";

export const DATA_SOURCE: DataSource = process.env.NEXT_PUBLIC_DATA_SOURCE === "rpc" ? "rpc" : "mock";

export const RPC_URL = process.env.NEXT_PUBLIC_RPC_URL ?? "http://127.0.0.1:8899";

/** Placeholder until `anchor keys sync` in ../program produces the real id. */
export const PROGRAM_ID = new PublicKey(process.env.NEXT_PUBLIC_PROGRAM_ID ?? "2GL5kpBSr1aAM6wHveCgeUD1AkzJ4qwMa5MVaSdC1ND7");

export const USDC_MINT = new PublicKey(process.env.NEXT_PUBLIC_USDC_MINT ?? "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");

export const TOKEN_PROGRAM_ID = new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
export const ASSOCIATED_TOKEN_PROGRAM_ID = new PublicKey("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
export const SYSTEM_PROGRAM_ID = new PublicKey("11111111111111111111111111111111");

/** Keeper status server (keeper/README.md, `status_bind`). Used by /ops in rpc mode. */
export const KEEPER_URL = (process.env.NEXT_PUBLIC_KEEPER_URL ?? "http://127.0.0.1:8787").replace(/\/$/, "");

/** Wallets allowed to open /ops in rpc mode. Empty means nobody; mock mode is always open. */
export const OPS_ALLOWED_WALLETS: string[] = (process.env.NEXT_PUBLIC_OPS_ALLOWED_WALLETS ?? "")
  .split(",")
  .map((s) => s.trim())
  .filter(Boolean);
