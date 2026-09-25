// Force market_open and wind every eligible vault into Basis now (hackathon demo; keeper-signed).
// The program still enforces the rule in wind_start; ineligible vaults are reported, not forced.
//   ANCHOR_PROVIDER_URL=<rpc> ANCHOR_WALLET=~/.config/carrera/keeper.json tsx wind-now.ts
import anchor from "@coral-xyz/anchor";
import { PublicKey } from "@solana/web3.js";
import { readFileSync } from "node:fs";
const idl = JSON.parse(readFileSync(new URL("../program/target/idl/carrera_overlay.json", import.meta.url), "utf8"));
const cfg = JSON.parse(readFileSync(new URL("./vaults.json", import.meta.url), "utf8"));
const provider = anchor.AnchorProvider.env();
anchor.setProvider(provider);
const program = new anchor.Program(idl, provider) as anchor.Program;
const keeper = provider.wallet.publicKey;
const STATE = ["idle", "parked", "winding", "basis", "unwinding"];

for (const v of cfg.vaults as { symbol: string; mint: string }[]) {
  const [vault] = PublicKey.findProgramAddressSync([Buffer.from("vault"), new PublicKey(v.mint).toBuffer()], program.programId);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const read = async () => (program.account as any).overlayVault.fetch(vault);
  let a = await read();
  if (STATE[a.state] !== "idle" && STATE[a.state] !== "parked") { console.log(`${v.symbol.padEnd(6)} already ${STATE[a.state]}`); continue; }
  try {
    if (!a.marketOpen) await program.methods.setMarketOpen(true).accounts({ keeper, vault }).rpc();
    await program.methods.windStart().accounts({ keeper, vault }).rpc();
    for (const n of [1, 2, 3]) await program.methods.windStep(n).accounts({ keeper, vault }).rpc();
    await program.methods.windCommit().accounts({ keeper, vault }).rpc();
    a = await read();
    console.log(`${v.symbol.padEnd(6)} -> ${STATE[a.state]}  debt=${(Number(a.debtUsdc) / 1e6).toFixed(2)} USDC  short=${a.phoenixShortQty.toString()}  f_avg=${a.lastRule.fAvgBps.toString()} hurdle=${a.lastRule.hurdleBps.toString()}`);
  } catch (e) {
    const msg = String((e as Error).message ?? e);
    const m = msg.match(/Error Code: (\w+)/);
    console.log(`${v.symbol.padEnd(6)} stays ${STATE[(await read()).state]}: ${m ? m[1] : msg.slice(0, 120)}`);
  }
}
