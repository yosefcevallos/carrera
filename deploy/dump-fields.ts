import anchor from "@coral-xyz/anchor"; import { PublicKey } from "@solana/web3.js"; import { readFileSync } from "node:fs";
const idl = JSON.parse(readFileSync("/Users/yosefcevallos/Desktop/carrera/program/target/idl/carrera_overlay.json","utf8"));
const provider = anchor.AnchorProvider.env(); anchor.setProvider(provider);
const program = new anchor.Program(idl, provider) as any;
const cfg = JSON.parse(readFileSync("/Users/yosefcevallos/Desktop/carrera/deploy/vaults.json","utf8"));
(async () => {
  const v = cfg.vaults.find((x: any) => x.symbol === (process.env.ONLY ?? "SPY"));
  const [vault] = PublicKey.findProgramAddressSync([Buffer.from("vault"), new PublicKey(v.mint).toBuffer()], program.programId);
  const a = await program.account.overlayVault.fetch(vault);
  for (const [k, x] of Object.entries(a)) { if (typeof x === "object" && x !== null && "toString" in x && !(x instanceof PublicKey) && !Array.isArray(x)) { const s = (x as any).toString(); if (/^-?\d+$/.test(s)) console.log(k, s); } else if (typeof x === "number" || typeof x === "boolean") console.log(k, x); }
})();
