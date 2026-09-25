// Set every vault's deposit cap (in stock, at the live price) and basis cap (USDC) to CAP_USD (admin).
// Reads current params from chain and changes only deposit_cap_stock and basis_cap_usdc.
//   ANCHOR_PROVIDER_URL=<rpc> ANCHOR_WALLET=~/.config/solana/id.json CAP_USD=1000 tsx set-caps.ts
import anchor from "@coral-xyz/anchor";
import { PublicKey } from "@solana/web3.js";
import { readFileSync } from "node:fs";
const { BN } = anchor;
const idl = JSON.parse(readFileSync(new URL("../program/target/idl/carrera_overlay.json", import.meta.url), "utf8"));
const cfg = JSON.parse(readFileSync(new URL("./vaults.json", import.meta.url), "utf8"));
const provider = anchor.AnchorProvider.env();
anchor.setProvider(provider);
const program = new anchor.Program(idl, provider) as anchor.Program;
const capUsd = Number(process.env.CAP_USD ?? "1000");

async function prices(mints: string[]): Promise<Record<string, number>> {
  const r = await fetch(`https://lite-api.jup.ag/price/v3?ids=${mints.join(",")}`);
  const j = r.ok ? ((await r.json()) as Record<string, { usdPrice: number } | undefined>) : {};
  const out: Record<string, number> = {};
  for (const m of mints) { const p = j[m]?.usdPrice; if (p) out[m] = p; }
  return out;
}

const vaults = cfg.vaults as { symbol: string; mint: string }[];
const px = await prices(vaults.map((v) => v.mint));
for (const v of vaults) {
  if (!px[v.mint]) { console.log(`${v.symbol.padEnd(6)} no price, skipped`); continue; }
  const [vault] = PublicKey.findProgramAddressSync([Buffer.from("vault"), new PublicKey(v.mint).toBuffer()], program.programId);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const acct = await (program.account as any).overlayVault.fetch(vault);
  const decimals = Number(acct.stockDecimals) || 8;
  const depositCap = BigInt(Math.floor((capUsd / px[v.mint]) * 10 ** decimals));
  const basisCap = BigInt(Math.round(capUsd)) * 1_000_000n;
  const params = { ...acct.params, depositCapStock: new BN(depositCap.toString()), basisCapUsdc: new BN(basisCap.toString()) };
  const sig = await program.methods.setParams(params).accounts({ admin: provider.wallet.publicKey, vault }).rpc();
  console.log(`${v.symbol.padEnd(6)} deposit cap ${(Number(depositCap) / 10 ** decimals).toFixed(4)} ${v.symbol}x ($${capUsd} at $${px[v.mint].toFixed(2)})  basis cap $${capUsd}  ${sig.slice(0, 10)}…`);
}
