// Deposit xStocks held by ANCHOR_WALLET into their vaults. Amount per vault from DEPOSIT_UI (UI units, default 1).
// Deposits the lesser of DEPOSIT_UI and the wallet balance; skips vaults where the wallet holds none.
//   ANCHOR_PROVIDER_URL=<rpc> ANCHOR_WALLET=~/.config/solana/id.json DEPOSIT_UI=1 tsx deposit.ts
import anchor from "@coral-xyz/anchor";
import { PublicKey } from "@solana/web3.js";
import {
  TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID, getAssociatedTokenAddressSync, getAccount,
  createAssociatedTokenAccountIdempotentInstruction,
} from "@solana/spl-token";
import { readFileSync } from "node:fs";
const { BN } = anchor;

const idl = JSON.parse(readFileSync(new URL("../program/target/idl/carrera_overlay.json", import.meta.url), "utf8"));
const cfg = JSON.parse(readFileSync(new URL("./vaults.json", import.meta.url), "utf8"));
const provider = anchor.AnchorProvider.env();
anchor.setProvider(provider);
const program = new anchor.Program(idl, provider) as anchor.Program;
const user = provider.wallet.publicKey;
const want = Number(process.env.DEPOSIT_UI ?? "1");

for (const v of cfg.vaults as { symbol: string; mint: string }[]) {
  const mint = new PublicKey(v.mint);
  const userStock = getAssociatedTokenAddressSync(mint, user, false, TOKEN_2022_PROGRAM_ID);
  let bal = 0n;
  try { bal = (await getAccount(provider.connection, userStock, "confirmed", TOKEN_2022_PROGRAM_ID)).amount; } catch { /* no account */ }
  if (bal === 0n) { console.log(`${v.symbol.padEnd(6)} no ${v.symbol}x in wallet, skipped`); continue; }
  const qty = bal < BigInt(Math.round(want * 1e8)) ? bal : BigInt(Math.round(want * 1e8));
  const [vault] = PublicKey.findProgramAddressSync([Buffer.from("vault"), mint.toBuffer()], program.programId);
  const [shareMint] = PublicKey.findProgramAddressSync([Buffer.from("shares"), vault.toBuffer()], program.programId);
  const userShares = getAssociatedTokenAddressSync(shareMint, user, false, TOKEN_PROGRAM_ID);
  const sig = await program.methods
    .deposit(new BN(qty.toString()), new BN(0))
    .accounts({ user, vault, xstockMint: mint, userStock, userShares, tokenProgram: TOKEN_PROGRAM_ID, stockTokenProgram: TOKEN_2022_PROGRAM_ID })
    .preInstructions([createAssociatedTokenAccountIdempotentInstruction(user, userShares, user, shareMint, TOKEN_PROGRAM_ID)])
    .rpc();
  console.log(`${v.symbol.padEnd(6)} deposited ${(Number(qty) / 1e8).toFixed(4)} ${v.symbol}x  ${sig.slice(0, 12)}…`);
}
