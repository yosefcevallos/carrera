// Print the PDA table for vaults.json (no RPC needed).
import { PublicKey } from "@solana/web3.js";
import { readFileSync } from "node:fs";
const cfg = JSON.parse(readFileSync(new URL(process.env.VAULTS_JSON ?? "./vaults.json", import.meta.url), "utf8"));
const PROGRAM = new PublicKey(process.env.PROGRAM_ID ?? "GH45ANLzg1t6rNnaoNqE39rN1rKGXFQXPZnNxvbhUmYw");
const [registry] = PublicKey.findProgramAddressSync([Buffer.from("registry")], PROGRAM);
const TIER: Record<string, number> = { A: 0, B: 1, C: 2, D: 3 };
const rows = (cfg.vaults as { symbol: string; tier: string; mint: string }[]).filter((v) => v.mint).map((v) => {
  const mint = new PublicKey(v.mint);
  const [vault] = PublicKey.findProgramAddressSync([Buffer.from("vault"), mint.toBuffer()], PROGRAM);
  const [shares] = PublicKey.findProgramAddressSync([Buffer.from("shares"), vault.toBuffer()], PROGRAM);
  return { symbol: v.symbol, tier: TIER[v.tier], xstock_mint: v.mint, vault_pubkey: vault.toBase58(), share_mint: shares.toBase58() };
});
console.log(JSON.stringify({ program_id: PROGRAM.toBase58(), registry: registry.toBase58(), vaults: rows }, null, 1));
