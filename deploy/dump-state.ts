import anchor from "@coral-xyz/anchor"; import { PublicKey } from "@solana/web3.js"; import { readFileSync } from "node:fs";
const idl = JSON.parse(readFileSync("/Users/yosefcevallos/Desktop/carrera/program/target/idl/carrera_overlay.json","utf8"));
const provider = anchor.AnchorProvider.env(); anchor.setProvider(provider);
const program = new anchor.Program(idl, provider) as any;
const cfg = JSON.parse(readFileSync("/Users/yosefcevallos/Desktop/carrera/deploy/vaults.json","utf8"));
const only = (process.env.ONLY ?? "").split(",").filter(Boolean);
const pick = (o:any, ks:string[]) => Object.fromEntries(ks.map(k => [k, o[k]?.toString?.() ?? o[k]]));
(async () => {
for (const v of cfg.vaults) {
  if (only.length && !only.includes(v.symbol)) continue;
  const mint = new PublicKey(v.mint);
  const [vault] = PublicKey.findProgramAddressSync([Buffer.from("vault"), mint.toBuffer()], program.programId);
  const a = await program.account.overlayVault.fetch(vault);
  console.log(v.symbol, pick(a, ["state","step","epochId","epochOpenedTs","pendingExitShares","totalShares","navSlot","marketOpen","phoenixShortQty","kaminoDebtUsdc","kaminoParkedUsdc","usdcBufferHint"]), "epoch_len", a.params.epochLenSecs.toString(), "now", Math.floor(Date.now()/1000));
  if (process.env.EPOCHS) for (let id = 0; id <= Number(a.epochId); id++) { const b = Buffer.alloc(8); b.writeBigUInt64LE(BigInt(id)); const [ep] = PublicKey.findProgramAddressSync([Buffer.from("epoch"), vault.toBuffer(), b], program.programId);
    try { const e = await program.account.exitEpoch.fetch(ep); console.log("  epoch", id, JSON.stringify(Object.fromEntries(Object.entries(e).map(([k,x]:any) => [k, x?.toString?.() ?? x])))); } catch { console.log("  epoch", id, "missing"); } }
}
})();
