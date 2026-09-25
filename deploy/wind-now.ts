// Refresh NAV on every vault, force market_open, and wind every funded, eligible vault into Basis (keeper-signed).
// Resumable: a vault already in Winding continues from its current step. Retries stale-preflight errors.
// The program still enforces the rule in wind_start; ineligible or empty vaults are reported, not forced.
//   ANCHOR_PROVIDER_URL=<rpc> ANCHOR_WALLET=~/.config/carrera/keeper.json tsx wind-now.ts
import anchor from "@coral-xyz/anchor";
import { PublicKey, SystemProgram } from "@solana/web3.js";
import { readFileSync } from "node:fs";
const { BN } = anchor;
const idl = JSON.parse(readFileSync(new URL("../program/target/idl/carrera_overlay.json", import.meta.url), "utf8"));
const cfg = JSON.parse(readFileSync(new URL("./vaults.json", import.meta.url), "utf8"));
const provider = anchor.AnchorProvider.env();
anchor.setProvider(provider);
const program = new anchor.Program(idl, provider) as anchor.Program;
const keeper = provider.wallet.publicKey;
const STATE = ["idle", "parked", "winding", "basis", "unwinding"];
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function prices(mints: string[]): Promise<Record<string, number>> {
  const r = await fetch(`https://lite-api.jup.ag/price/v3?ids=${mints.join(",")}`);
  const j = r.ok ? ((await r.json()) as Record<string, { usdPrice: number } | undefined>) : {};
  const out: Record<string, number> = {};
  for (const m of mints) { const p = j[m]?.usdPrice; if (p) out[m] = Math.round(p * 1e6); }
  return out;
}

// Retry on the errors a stale preflight node produces (state/step not yet visible).
async function send(label: string, f: () => Promise<string>): Promise<void> {
  for (let i = 0; i < 4; i++) {
    try { await f(); return; } catch (e) {
      const msg = String((e as Error).message ?? e);
      if (i < 3 && /WrongState|WrongStep|MarketClosed|NavStale/.test(msg)) { await sleep(2500); continue; }
      throw new Error(`${label}: ${msg}`);
    }
  }
}

const vaults = cfg.vaults as { symbol: string; mint: string }[];
const px = await prices(vaults.map((v) => v.mint));

// 1. Refresh NAV everywhere so deposits and winds see a fresh price.
for (const v of vaults) {
  const [vault] = PublicKey.findProgramAddressSync([Buffer.from("vault"), new PublicKey(v.mint).toBuffer()], program.programId);
  if (!px[v.mint]) { console.log(`${v.symbol.padEnd(6)} no price, NAV not refreshed`); continue; }
  try {
    await send("refresh_nav", () => program.methods.refreshNav(new BN(px[v.mint])).accounts({ signer: keeper, vault, oracle: SystemProgram.programId }).rpc());
    console.log(`${v.symbol.padEnd(6)} nav refreshed at $${(px[v.mint] / 1e6).toFixed(2)}`);
  } catch (e) { console.log(`${v.symbol.padEnd(6)} refresh_nav failed: ${String((e as Error).message).slice(0, 120)}`); }
}

// 2. Wind funded vaults.
for (const v of vaults) {
  const [vault] = PublicKey.findProgramAddressSync([Buffer.from("vault"), new PublicKey(v.mint).toBuffer()], program.programId);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const read = async () => (program.account as any).overlayVault.fetch(vault);
  let a = await read();
  const st = STATE[a.state];
  if (st === "basis" || st === "unwinding") { console.log(`${v.symbol.padEnd(6)} already ${st}`); continue; }
  if (st !== "winding" && a.collateralQty.toString() === "0") { console.log(`${v.symbol.padEnd(6)} stays idle: no deposits yet`); continue; }
  try {
    if (st !== "winding") {
      if (!a.marketOpen) await send("set_market_open", () => program.methods.setMarketOpen(true).accounts({ keeper, vault }).rpc());
      await send("wind_start", () => program.methods.windStart().accounts({ keeper, vault }).rpc());
      a = await read();
    }
    for (let n = Number(a.step) + 1; n <= 3; n++) {
      await send(`wind_step(${n})`, () => program.methods.windStep(n).accounts({ keeper, vault }).rpc());
    }
    await send("wind_commit", () => program.methods.windCommit().accounts({ keeper, vault }).rpc());
    a = await read();
    console.log(`${v.symbol.padEnd(6)} -> ${STATE[a.state]}  nav=$${(Number(a.navUsdE6) / 1e6).toFixed(2)}  debt=${(Number(a.debtUsdc) / 1e6).toFixed(2)} USDC  short=${a.phoenixShortQty.toString()}  f_avg=${a.lastRule.fAvgBps.toString()} hurdle=${a.lastRule.hurdleBps.toString()}`);
  } catch (e) {
    const msg = String((e as Error).message ?? e);
    const m = msg.match(/Error Code: (\w+)/);
    a = await read();
    console.log(`${v.symbol.padEnd(6)} stays ${STATE[a.state]} (step ${a.step}): ${m ? m[1] : msg.slice(0, 200)}`);
  }
}
