// Exercise the app's own deposit / request_exit builders against devnet with a keypair.
//   NEXT_PUBLIC_PROGRAM_ID=<id> NEXT_PUBLIC_XSTOCK_MINTS='{...}' NEXT_PUBLIC_RPC_URL=https://api.devnet.solana.com \
//   pnpm tsx scripts/devnet-flow.ts TSLA 0.25 [~/.config/solana/id.json]
import { Connection, Keypair } from "@solana/web3.js";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import type { Ticker } from "../src/constants/vaults";

const [ticker = "TSLA", amountArg = "0.25", keyPath = `${homedir()}/.config/solana/id.json`] = process.argv.slice(2);
const amount = Number(amountArg);
const kp = Keypair.fromSecretKey(Uint8Array.from(JSON.parse(readFileSync(keyPath, "utf8"))));
const rpc = process.env.NEXT_PUBLIC_RPC_URL ?? "https://api.devnet.solana.com";
const conn = new Connection(rpc, "confirmed");
const signer = {
  publicKey: kp.publicKey,
  sendTransaction: async (tx: import("@solana/web3.js").Transaction) => {
    tx.sign(kp);
    return conn.sendRawTransaction(tx.serialize(), { skipPreflight: false });
  },
};

async function main() {
  const { buildDepositTx, buildRequestExitTx, sendAndConfirm, stockDecimals } = await import("../src/lib/chain/actions");
  const t = ticker as Ticker;
  console.log("cluster", rpc, "wallet", kp.publicKey.toBase58(), "decimals", await stockDecimals(conn, t));
  const dep = await sendAndConfirm(conn, signer, await buildDepositTx(conn, kp.publicKey, t, amount));
  console.log(`deposit ${amount} ${t}x  ${dep}`);
  const ex = await buildRequestExitTx(conn, kp.publicKey, t, amount / 2);
  const exSig = await sendAndConfirm(conn, signer, ex);
  console.log(`request_exit ${amount / 2} shares nonce=${ex.nonce} epoch=${ex.epochId}  ${exSig}`);
}

main().catch((e) => { console.error(e); process.exit(1); });
