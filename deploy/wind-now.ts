// Refresh NAV, force market_open, and wind every eligible vault into Basis now (hackathon demo; keeper-signed).
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

async function priceE6(mint: string): Promise<number> {
  const r = await fetch(`https://lite-api.jup.ag/price/v3?ids=${mint}`);
  const j = r.ok ? ((await r.json()) as Record<string, { usdPrice: number } | undefined>) : {};
  const p = j[mint]?.usdPrice;
  if (!p) throw new Error(`no Jupiter price for ${mint}`);
  return Math.round(p * 1e6);
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

for (const v of cfg.vaults as { symbol: string; mint: string }[]) {
  const [vault] = PublicKey.findProgramAddressSync([Buffer.from("vault"), new PublicKey(v.mint).toBuffer()], program.programId);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const read = async () => (program.account as any).overlayVault.fetch(vault);
  let a = await read();
  const st = STATE[a.state];
  if (st === "basis" || st === "unwinding") { console.log(`${v.symbol.padEnd(6)} already ${st}`); continue; }
  if (st !== "winding" && a.collateralQty.toString() === "0") { console.log(`${v.symbol.padEnd(6)} stays idle: no deposits yet`); continue; }
  try {
    if (st !== "winding") {
      const px = await priceE6(v.mint);
      await send("refresh_nav", () => program.methods.refreshNav(new BN(px)).accounts({ signer: keeper, vault, oracle: SystemProgram.programId }).rpc());
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
