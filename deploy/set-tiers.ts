// Apply DECISIONS.md D7 tiers to every vault (admin): ltv, liq ltv, emergency ltv, min margin.
// Reads current params from chain and changes only those four fields.
//   ANCHOR_PROVIDER_URL=<rpc> ANCHOR_WALLET=~/.config/solana/id.json tsx set-tiers.ts
import anchor from "@coral-xyz/anchor";
import { PublicKey } from "@solana/web3.js";
import { readFileSync } from "node:fs";
const idl = JSON.parse(readFileSync(new URL("../program/target/idl/carrera_overlay.json", import.meta.url), "utf8"));
const cfg = JSON.parse(readFileSync(new URL("./vaults.json", import.meta.url), "utf8"));
const provider = anchor.AnchorProvider.env();
anchor.setProvider(provider);
const program = new anchor.Program(idl, provider) as anchor.Program;

// D7: L = 0.6 × Kamino liq LTV (capped at Kamino max LTV); emergency = liq − 500; min margin per Phoenix tier.
const TIERS: Record<string, { ltv: number; liq: number; minMargin: number }> = {
  SPY:   { ltv: 4500, liq: 7500, minMargin: 1000 },
  QQQ:   { ltv: 4300, liq: 7200, minMargin: 1000 },
  GOOGL: { ltv: 4200, liq: 7000, minMargin: 1000 },
  TSLA:  { ltv: 3900, liq: 6500, minMargin: 1000 },
  NVDA:  { ltv: 3900, liq: 6500, minMargin: 1000 },
  AAPL:  { ltv: 3000, liq: 5000, minMargin: 1000 },
  MSTR:  { ltv: 2400, liq: 4000, minMargin: 1200 },
  CRCL:  { ltv: 2400, liq: 4000, minMargin: 1200 },
  HOOD:  { ltv: 2400, liq: 4000, minMargin: 1200 },
};

for (const v of cfg.vaults as { symbol: string; mint: string }[]) {
  const t = TIERS[v.symbol];
  if (!t) { console.log(`${v.symbol.padEnd(6)} no tier, skipped`); continue; }
  const [vault] = PublicKey.findProgramAddressSync([Buffer.from("vault"), new PublicKey(v.mint).toBuffer()], program.programId);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const acct = await (program.account as any).overlayVault.fetch(vault);
  const params = { ...acct.params, ltvBps: t.ltv, liqLtvBps: t.liq, emergencyLtvBps: t.liq - 500, minMarginBps: t.minMargin };
  const sig = await program.methods.setParams(params).accounts({ admin: provider.wallet.publicKey, vault }).rpc();
  console.log(`${v.symbol.padEnd(6)} ltv ${acct.params.ltvBps}→${t.ltv}  liq ${acct.params.liqLtvBps}→${t.liq}  emergency →${t.liq - 500}  min_margin ${acct.params.minMarginBps}→${t.minMargin}  ${sig.slice(0, 10)}…`);
}
