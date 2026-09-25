#!/usr/bin/env bash
# Send AMOUNT USDC (default 0.1) from the CLI wallet to every vault's usdc_buffer token account.
# Needed before real-legs settlement: Kamino repay-all rounding and mock-era dust are paid from the buffer.
#   ./seed-buffers.sh [amount]
set -euo pipefail
cd "$(dirname "$0")"
export PATH=~/.local/share/solana/install/releases/3.1.1/solana-release/bin:$PATH
RPC="$(grep '^HELIUS_RPC_URL=' ../.env.supabase | cut -d= -f2-)"
AMOUNT="${1:-0.1}"
USDC=EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v
./node_modules/.bin/tsx -e '
import { PublicKey } from "@solana/web3.js"; import { readFileSync } from "node:fs";
const cfg = JSON.parse(readFileSync("vaults.json","utf8")); const P = new PublicKey("GH45ANLzg1t6rNnaoNqE39rN1rKGXFQXPZnNxvbhUmYw");
for (const v of cfg.vaults) { const [vault] = PublicKey.findProgramAddressSync([Buffer.from("vault"), new PublicKey(v.mint).toBuffer()], P); const [buf] = PublicKey.findProgramAddressSync([Buffer.from("usdc"), vault.toBuffer()], P); console.log(v.symbol, buf.toBase58()); }' |
while read -r sym buf; do
  printf "%-6s %s  " "$sym" "$buf"
  spl-token transfer "$USDC" "$AMOUNT" "$buf" -u "$RPC" --with-compute-unit-price 50000 2>&1 | grep -E "Signature|Error" | head -1
done
echo "done; balances:"
./node_modules/.bin/tsx -e '
import { Connection, PublicKey } from "@solana/web3.js"; import { readFileSync } from "node:fs";
const c = new Connection(process.env.RPC!); const cfg = JSON.parse(readFileSync("vaults.json","utf8")); const P = new PublicKey("GH45ANLzg1t6rNnaoNqE39rN1rKGXFQXPZnNxvbhUmYw");
for (const v of cfg.vaults) { const [vault] = PublicKey.findProgramAddressSync([Buffer.from("vault"), new PublicKey(v.mint).toBuffer()], P); const [buf] = PublicKey.findProgramAddressSync([Buffer.from("usdc"), vault.toBuffer()], P); const b = await c.getTokenAccountBalance(buf); console.log(v.symbol, b.value.uiAmountString, "USDC"); }' RPC="$RPC"
