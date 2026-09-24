// Instruction builders per docs/CONTRACT.md. Discriminator = sha256("global:<name>")[0..8], args Borsh-encoded.
import { PublicKey, SystemProgram, TransactionInstruction } from "@solana/web3.js";
import { serialize, type Schema } from "borsh";
import { sha256 } from "@noble/hashes/sha2.js";
import { PROGRAM_ID, TOKEN_PROGRAM_ID } from "./config";
import { ata, pda } from "./pda";

export function discriminator(name: string): Uint8Array {
  return sha256(new TextEncoder().encode(`global:${name}`)).slice(0, 8);
}

function data(name: string, schema: Schema | null, args: unknown): Buffer {
  const disc = discriminator(name);
  if (!schema) return Buffer.from(disc);
  return Buffer.concat([Buffer.from(disc), Buffer.from(serialize(schema, args))]);
}

const meta = (pubkey: PublicKey, isWritable: boolean, isSigner = false) => ({ pubkey, isWritable, isSigner });

export interface VaultKeys {
  xstockMint: PublicKey;
  registry: PublicKey;
  vault: PublicKey;
  shareMint: PublicKey;
  stockCustody: PublicKey;
  escrowShares: PublicKey;
  redeemStock: PublicKey;
  redeemUsdc: PublicKey;
}

export function vaultKeys(xstockMint: PublicKey): VaultKeys {
  const vault = pda.vault(xstockMint);
  return {
    xstockMint,
    registry: pda.registry(),
    vault,
    shareMint: pda.shareMint(vault),
    stockCustody: pda.stockCustody(vault),
    escrowShares: pda.escrowShares(vault),
    redeemStock: pda.redeemStock(vault),
    redeemUsdc: pda.redeemUsdc(vault),
  };
}

const DepositArgs: Schema = { struct: { qty: "u64", min_shares: "u64" } };
const RequestExitArgs: Schema = { struct: { shares: "u64", nonce: "u64" } };

/** deposit(qty, min_shares) — stock-only at launch. */
export function depositIx(user: PublicKey, k: VaultKeys, qty: bigint, minShares: bigint): TransactionInstruction {
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      meta(user, true, true),
      meta(k.registry, false),
      meta(k.vault, true),
      meta(k.shareMint, true),
      meta(k.xstockMint, false),
      meta(k.stockCustody, true),
      meta(ata(user, k.xstockMint), true),
      meta(ata(user, k.shareMint), true),
      meta(TOKEN_PROGRAM_ID, false),
    ],
    data: data("deposit", DepositArgs, { qty, min_shares: minShares }),
  });
}

/** request_exit(shares, nonce) — escrows shares into the open epoch. */
export function requestExitIx(user: PublicKey, k: VaultKeys, shares: bigint, nonce: bigint, epochId: bigint): TransactionInstruction {
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      meta(user, true, true),
      meta(k.registry, false),
      meta(k.vault, true),
      meta(pda.exitRequest(k.vault, user, nonce), true),
      meta(pda.exitEpoch(k.vault, epochId), true),
      meta(k.shareMint, false),
      meta(ata(user, k.shareMint), true),
      meta(k.escrowShares, true),
      meta(TOKEN_PROGRAM_ID, false),
      meta(SystemProgram.programId, false),
    ],
    data: data("request_exit", RequestExitArgs, { shares, nonce }),
  });
}

/** cancel_exit — returns escrowed shares while the epoch is still open. */
export function cancelExitIx(user: PublicKey, k: VaultKeys, nonce: bigint, epochId: bigint): TransactionInstruction {
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      meta(user, true, true),
      meta(k.vault, true),
      meta(pda.exitRequest(k.vault, user, nonce), true),
      meta(pda.exitEpoch(k.vault, epochId), true),
      meta(k.shareMint, false),
      meta(ata(user, k.shareMint), true),
      meta(k.escrowShares, true),
      meta(TOKEN_PROGRAM_ID, false),
    ],
    data: data("cancel_exit", null, undefined),
  });
}

/** redeem — burns escrowed shares and pays stock + USDC from the settled epoch. */
export function redeemIx(user: PublicKey, k: VaultKeys, usdcMint: PublicKey, nonce: bigint, epochId: bigint): TransactionInstruction {
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [
      meta(user, true, true),
      meta(k.vault, true),
      meta(pda.exitRequest(k.vault, user, nonce), true),
      meta(pda.exitEpoch(k.vault, epochId), true),
      meta(k.shareMint, true),
      meta(k.escrowShares, true),
      meta(k.redeemStock, true),
      meta(k.redeemUsdc, true),
      meta(ata(user, k.xstockMint), true),
      meta(ata(user, usdcMint), true),
      meta(TOKEN_PROGRAM_ID, false),
    ],
    data: data("redeem", null, undefined),
  });
}

// ---- Ops cranks (keeper / guardian signed) per docs/CONTRACT.md ----------------------

const UnwindStartArgs: Schema = { struct: { reason: "u8" } };

/** UnwindReason per CONTRACT.md: Rule=0, ExitDemand=1, Emergency=2. */
export const UNWIND_EMERGENCY = 2;

/** rebalance_to_kamino — Basis only: Phoenix free collateral → repay Kamino debt. */
export function rebalanceToKaminoIx(keeper: PublicKey, k: VaultKeys): TransactionInstruction {
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [meta(keeper, false, true), meta(k.registry, false), meta(k.vault, true)],
    data: data("rebalance_to_kamino", null, undefined),
  });
}

/** rebalance_to_phoenix — Basis only: Kamino borrow → Phoenix margin. */
export function rebalanceToPhoenixIx(keeper: PublicKey, k: VaultKeys): TransactionInstruction {
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [meta(keeper, false, true), meta(k.registry, false), meta(k.vault, true)],
    data: data("rebalance_to_phoenix", null, undefined),
  });
}

/** unwind_start(reason) — keeper, or guardian for reason = Emergency. */
export function unwindStartIx(signer: PublicKey, k: VaultKeys, reason: number): TransactionInstruction {
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [meta(signer, false, true), meta(k.registry, false), meta(k.vault, true)],
    data: data("unwind_start", UnwindStartArgs, { reason }),
  });
}

/** pause — admin or guardian. */
export function pauseIx(signer: PublicKey, registry: PublicKey): TransactionInstruction {
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    keys: [meta(signer, false, true), meta(registry, true)],
    data: data("pause", null, undefined),
  });
}
