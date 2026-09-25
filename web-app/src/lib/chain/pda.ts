// PDAs per docs/CONTRACT.md.
import { PublicKey } from "@solana/web3.js";
import { ASSOCIATED_TOKEN_PROGRAM_ID, PROGRAM_ID, TOKEN_PROGRAM_ID } from "./config";

// Browser-safe u64 little-endian (the Buffer polyfill lacks writeBigUInt64LE).
const u64le = (n: bigint) => {
  const b = new Uint8Array(8);
  new DataView(b.buffer).setBigUint64(0, BigInt.asUintN(64, n), true);
  return b;
};

const find = (seeds: (Buffer | Uint8Array)[]) => PublicKey.findProgramAddressSync(seeds, PROGRAM_ID)[0];

export const pda = {
  registry: () => find([Buffer.from("registry")]),
  vault: (xstockMint: PublicKey) => find([Buffer.from("vault"), xstockMint.toBuffer()]),
  shareMint: (vault: PublicKey) => find([Buffer.from("shares"), vault.toBuffer()]),
  stockCustody: (vault: PublicKey) => find([Buffer.from("stock"), vault.toBuffer()]),
  usdcBuffer: (vault: PublicKey) => find([Buffer.from("usdc"), vault.toBuffer()]),
  redeemStock: (vault: PublicKey) => find([Buffer.from("redeem_stock"), vault.toBuffer()]),
  redeemUsdc: (vault: PublicKey) => find([Buffer.from("redeem_usdc"), vault.toBuffer()]),
  escrowShares: (vault: PublicKey) => find([Buffer.from("escrow"), vault.toBuffer()]),
  exitRequest: (vault: PublicKey, user: PublicKey, nonce: bigint) =>
    find([Buffer.from("exit"), vault.toBuffer(), user.toBuffer(), u64le(nonce)]),
  exitEpoch: (vault: PublicKey, epochId: bigint) => find([Buffer.from("epoch"), vault.toBuffer(), u64le(epochId)]),
};

export function ata(owner: PublicKey, mint: PublicKey, tokenProgram: PublicKey = TOKEN_PROGRAM_ID): PublicKey {
  return PublicKey.findProgramAddressSync(
    [owner.toBuffer(), tokenProgram.toBuffer(), mint.toBuffer()],
    ASSOCIATED_TOKEN_PROGRAM_ID,
  )[0];
}
