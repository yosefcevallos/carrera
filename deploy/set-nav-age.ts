// Raise max_nav_age_slots on every vault (admin). Reads current params from chain, changes one field.
//   ANCHOR_PROVIDER_URL=<rpc> ANCHOR_WALLET=~/.config/solana/id.json NAV_AGE_SLOTS=9000 tsx set-nav-age.ts
import anchor from "@coral-xyz/anchor";
import { PublicKey } from "@solana/web3.js";
import { readFileSync } from "node:fs";
const { BN } = anchor;
const idl = JSON.parse(readFileSync(new URL("../program/target/idl/carrera_overlay.json", import.meta.url), "utf8"));
const cfg = JSON.parse(readFileSync(new URL("./vaults.json", import.meta.url), "utf8"));
const provider = anchor.AnchorProvider.env();
anchor.setProvider(provider);
const program = new anchor.Program(idl, provider) as anchor.Program;
const slots = new BN(process.env.NAV_AGE_SLOTS ?? "9000");
for (const v of cfg.vaults as { symbol: string; mint: string }[]) {
  const [vault] = PublicKey.findProgramAddressSync([Buffer.from("vault"), new PublicKey(v.mint).toBuffer()], program.programId);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const acct = await (program.account as any).overlayVault.fetch(vault);
  const params = { ...acct.params, maxNavAgeSlots: slots };
  const sig = await program.methods.setParams(params).accounts({ admin: provider.wallet.publicKey, vault }).rpc();
  console.log(`${v.symbol.padEnd(6)} max_nav_age_slots ${acct.params.maxNavAgeSlots.toString()} -> ${slots.toString()}  ${sig.slice(0, 12)}…`);
}
