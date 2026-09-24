// Backfill each vault's 24-sample funding ring from Phoenix's last 24 hourly rates.
// Mock-venues build only (sample spacing is 0 there). Signed by a registered keeper.
//   ANCHOR_PROVIDER_URL=<rpc> ANCHOR_WALLET=~/.config/carrera/keeper.json tsx backfill-funding.ts
import anchor from "@coral-xyz/anchor";
import { PublicKey, SystemProgram } from "@solana/web3.js";
import { readFileSync } from "node:fs";
const { BN } = anchor;

const idl = JSON.parse(readFileSync(new URL("../program/target/idl/carrera_overlay.json", import.meta.url), "utf8"));
const cfg = JSON.parse(readFileSync(new URL("./vaults.json", import.meta.url), "utf8"));
const provider = anchor.AnchorProvider.env();
anchor.setProvider(provider);
const program = new anchor.Program(idl, provider);
const FUNDING_SCALE = 1_000_000;

async function rates(symbol: string): Promise<{ ts: number; scaled: number }[]> {
  const r = await fetch(`https://perp-api.phoenix.trade/v1/funding/${symbol}/rates?limit=24`);
  const j = (await r.json()) as { rates: { timestamp: number; fundingRatePercentage: string }[] };
  return j.rates
    .map((x) => ({ ts: x.timestamp, scaled: Math.round(Number(x.fundingRatePercentage) * 100 * FUNDING_SCALE) }))
    .sort((a, b) => a.ts - b.ts);
}

for (const v of cfg.vaults as { symbol: string; mint: string; phoenix_market?: string }[]) {
  const symbol = (v.phoenix_market ?? v.symbol).replace("-PERP", "");
  const rs = await rates(symbol);
  const [vault] = PublicKey.findProgramAddressSync([Buffer.from("vault"), new PublicKey(v.mint).toBuffer()], program.programId);
  let sent = 0;
  for (const r of rs) {
    await program.methods.recordFunding(new BN(r.scaled)).accounts({ signer: provider.wallet.publicKey, vault, hawkeyeView: SystemProgram.programId }).rpc();
    sent++;
  }
  const avgBps = rs.reduce((a, r) => a + r.scaled, 0) / rs.length * 8760 / FUNDING_SCALE;
  console.log(`${v.symbol.padEnd(6)} ${sent} samples, f_avg ≈ ${avgBps.toFixed(0)} bps`);
}
