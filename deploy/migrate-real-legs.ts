// Mock → real venue legs migration (program/README.md "Mainnet migration"). Resumable: every step
// is decided from the on-chain state, so re-running continues where the last run stopped.
//
//   --phase pre   (mock program still deployed) for each vault: refresh_nav (Jupiter price),
//                 set_market_open, Basis → unwind_start(EMERGENCY, guardian) → unwind_step 1..3 →
//                 unwind_commit → Parked → repay → Idle; mock debt dust → mock_accrue + repay.
//                 Vaults with pending exits are reported: run settle-now.ts for them.
//   --phase post  (real build deployed, keeper.toml has program_build = "real") for each vault:
//                 `carrera-keeper venues setup` (Kamino obligation, Phoenix collateral account,
//                 Phoenix trader registration + onboarding), then `venues sync-collateral`, then
//                 `venues check`.
//   --dry-run     print what would be sent, send nothing.   --only TSLA,NVDA   restrict vaults.
//
//   ANCHOR_PROVIDER_URL=<rpc> ANCHOR_WALLET=~/.config/carrera/keeper.json \
//   GUARDIAN_WALLET=~/.config/solana/id.json tsx migrate-real-legs.ts --phase pre [--dry-run]
//   KEEPER_BIN=../keeper/target/release/carrera-keeper KEEPER_CONFIG=~/.config/carrera/keeper.toml \
//   ANCHOR_PROVIDER_URL=<rpc> ANCHOR_WALLET=~/.config/carrera/keeper.json tsx migrate-real-legs.ts --phase post
//
// IDL: ../program/target/idl/carrera_overlay.json (the real-legs build). Its trailing `venue_data`
// argument is sent empty; the deployed mock program ignores trailing instruction bytes, the real
// build requires the argument, so the same encoding serves both phases.
import anchor from "@coral-xyz/anchor";
import { Keypair, PublicKey, SystemProgram } from "@solana/web3.js";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
const { BN } = anchor;

const args = process.argv.slice(2);
const flag = (name: string) => args.includes(name);
const opt = (name: string) => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : undefined; };
const phase = opt("--phase");
if (phase !== "pre" && phase !== "post") { console.error("usage: tsx migrate-real-legs.ts --phase pre|post [--dry-run] [--only SYM,SYM]"); process.exit(2); }
const dryRun = flag("--dry-run");
const only = opt("--only")?.split(",").map((s) => s.trim().toUpperCase());

const idlPath = new URL(process.env.IDL ?? "../program/target/idl/carrera_overlay.json", import.meta.url);
const idl = JSON.parse(readFileSync(idlPath, "utf8"));
const cfg = JSON.parse(readFileSync(new URL("./vaults.json", import.meta.url), "utf8"));
const provider = anchor.AnchorProvider.env();
anchor.setProvider(provider);
const program = new anchor.Program(idl, provider) as anchor.Program;
const keeper = provider.wallet.publicKey;
const STATE = ["idle", "parked", "winding", "basis", "unwinding"];
const EMERGENCY = 2;
const hasVenueArg = (name: string) => (idl.instructions as { name: string; args: unknown[] }[]).find((i) => i.name === name)!.args.some((a) => (a as { name: string }).name === "venue_data");
const venue = (name: string) => (hasVenueArg(name) ? [Buffer.alloc(0)] : []);
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const expand = (p: string) => p.replace(/^~/, homedir());

type VaultCfg = { symbol: string; mint: string };
const vaults = (cfg.vaults as VaultCfg[]).filter((v) => v.mint && (!only || only.includes(v.symbol.toUpperCase())));

async function priceE6(mint: string): Promise<number> {
  const r = await fetch(`https://lite-api.jup.ag/price/v3?ids=${mint}`);
  const j = r.ok ? ((await r.json()) as Record<string, { usdPrice: number } | undefined>) : {};
  const p = j[mint]?.usdPrice;
  if (!p) throw new Error(`no Jupiter price for ${mint}`);
  return Math.round(p * 1e6);
}

// Retry the errors a stale preflight node produces (state/step not yet visible).
async function send(label: string, f: () => Promise<string>): Promise<void> {
  if (dryRun) { console.log(`      would send ${label}`); return; }
  for (let i = 0; i < 4; i++) {
    try { const sig = await f(); console.log(`      ${label} ${sig.slice(0, 12)}…`); return; } catch (e) {
      const msg = String((e as Error).message ?? e);
      if (i < 3 && /WrongState|WrongStep|MarketClosed|NavStale|Blockhash not found/.test(msg)) { await sleep(2500); continue; }
      throw new Error(`${label}: ${msg}`);
    }
  }
}

function vaultPda(mint: string): PublicKey {
  return PublicKey.findProgramAddressSync([Buffer.from("vault"), new PublicKey(mint).toBuffer()], program.programId)[0];
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
const readVault = (vault: PublicKey) => (program.account as any).overlayVault.fetch(vault);
// eslint-disable-next-line @typescript-eslint/no-explicit-any
const readRegistry = () => (program.account as any).registry.fetch(PublicKey.findProgramAddressSync([Buffer.from("registry")], program.programId)[0]);

function summary(a: Record<string, unknown>): string {
  const n = (k: string) => (a[k] as { toString(): string }).toString();
  return `${STATE[a.state as number]} step ${a.step} collateral ${n("collateralQty")} spot ${n("basisSpotQty")} debt ${n("debtUsdc")} debt_b ${n("debtBUsdc")} parked ${n("parkedUsdc")} equity ${n("phoenixEquityUsdc")} short ${n("phoenixShortQty")} pending_exits ${n("pendingExitShares")}`;
}

// ------------------------------------------------------------------ pre: unwind on the mock program
async function pre(): Promise<void> {
  const reg = await readRegistry();
  const guardianPath = process.env.GUARDIAN_WALLET ?? process.env.ANCHOR_WALLET!;
  const guardianKp = Keypair.fromSecretKey(Uint8Array.from(JSON.parse(readFileSync(expand(guardianPath), "utf8"))));
  const guardianProvider = new anchor.AnchorProvider(provider.connection, new anchor.Wallet(guardianKp), {});
  const asGuardian = new anchor.Program(idl, guardianProvider) as anchor.Program;
  const guardianOk = guardianKp.publicKey.equals(reg.guardian as PublicKey);
  console.log(`registry guardian ${(reg.guardian as PublicKey).toBase58()}; GUARDIAN_WALLET ${guardianKp.publicKey.toBase58()} ${guardianOk ? "matches" : "DOES NOT MATCH (unwind_start will fail for healthy vaults)"}`);
  console.log(`keeper ${keeper.toBase58()}; ${dryRun ? "DRY RUN, nothing is sent" : "sending"}\n`);

  let clean = 0;
  for (const v of vaults) {
    const vault = vaultPda(v.mint);
    let a = await readVault(vault);
    console.log(`${v.symbol.padEnd(6)} ${summary(a)}`);
    if (a.pendingExitShares.toString() !== "0") console.log(`      pending exits: run settle-now.ts after this pass so they settle before the upgrade`);
    try {
      const px = await priceE6(v.mint);
      await send(`refresh_nav($${(px / 1e6).toFixed(2)})`, () => program.methods.refreshNav(new BN(px)).accounts({ signer: keeper, vault, oracle: SystemProgram.programId }).rpc());
      if (!a.marketOpen) await send("set_market_open(true)", () => program.methods.setMarketOpen(true).accounts({ keeper, vault }).rpc());
      if (STATE[a.state] === "winding") {
        await send("wind_abort", () => program.methods.windAbort().accounts({ keeper, vault }).rpc());
        if (!dryRun) a = await readVault(vault);
      }
      if (STATE[a.state] === "basis") {
        await send("unwind_start(emergency, guardian)", () => asGuardian.methods.unwindStart(EMERGENCY).accounts({ keeper: guardianKp.publicKey, vault }).rpc());
        if (!dryRun) a = await readVault(vault);
      }
      if (STATE[a.state] === "unwinding" || (dryRun && STATE[a.state] === "basis")) {
        const from = STATE[a.state] === "unwinding" ? Number(a.step) + 1 : 1;
        for (let n = from; n <= 3; n++) await send(`unwind_step(${n})`, () => program.methods.unwindStep(n, ...venue("unwind_step")).accounts({ keeper, vault }).rpc());
        await send("unwind_commit", () => program.methods.unwindCommit(...venue("unwind_commit")).accounts({ keeper, vault }).rpc());
        if (!dryRun) a = await readVault(vault);
      }
      if (STATE[a.state] === "parked" || (dryRun && STATE[a.state] !== "idle")) {
        await send("repay", () => program.methods.repay(...venue("repay")).accounts({ keeper, vault }).rpc());
        if (!dryRun) a = await readVault(vault);
      }
      // Mock-build residual: the mock unwind leaves debt dust that no real repayment can clear.
      // Credit it on the parked leg (mock_accrue exists only on the mock build) and repay from Idle.
      if (STATE[a.state] === "idle" && a.debtUsdc.toString() !== "0") {
        const dust = new BN(a.debtUsdc.toString());
        await send(`mock_accrue(${dust.toString()} dust)`, () => program.methods.mockAccrue(dust, 0).accounts({ keeper, vault }).rpc());
        await send("repay (dust)", () => program.methods.repay(...venue("repay")).accounts({ keeper, vault }).rpc());
        if (!dryRun) a = await readVault(vault);
      }
      if (!dryRun) {
        const ok = STATE[a.state] === "idle" && ["debtUsdc", "debtBUsdc", "phoenixShortQty", "phoenixEquityUsdc", "basisSpotQty"].every((k) => a[k].toString() === "0");
        console.log(`      → ${summary(a)} ${ok ? "CLEAN" : "NOT CLEAN"}`);
        if (ok) clean++;
      }
    } catch (e) {
      const msg = String((e as Error).message ?? e);
      const m = msg.match(/Error Code: (\w+)/);
      console.log(`      failed: ${m ? m[1] : msg.slice(0, 200)}`);
    }
  }
  if (!dryRun) console.log(`\n${clean}/${vaults.length} vaults Idle with no debt, short, equity or spot. Re-run until all are clean, then settle-now.ts, then upgrade.`);
}

// ------------------------------------------------------------------ post: obligations, trader, sync
function keeperCmd(...cmd: string[]): string {
  const bin = expand(process.env.KEEPER_BIN ?? "../keeper/target/release/carrera-keeper");
  const config = expand(process.env.KEEPER_CONFIG ?? "~/.config/carrera/keeper.toml");
  const full = [bin, "--config", config, ...cmd];
  if (dryRun && !cmd.includes("check")) { console.log(`      would run ${full.join(" ")}`); return ""; }
  return execFileSync(bin, ["--config", config, ...cmd], { encoding: "utf8", env: { ...process.env, CARRERA_RPC_URL: process.env.CARRERA_RPC_URL ?? process.env.ANCHOR_PROVIDER_URL } });
}

async function post(): Promise<void> {
  console.log(`keeper ${keeper.toBase58()}; ${dryRun ? "DRY RUN, nothing is sent" : "sending via carrera-keeper"}\n`);
  let synced = 0;
  for (const v of vaults) {
    const vault = vaultPda(v.mint);
    const a = await readVault(vault);
    console.log(`${v.symbol.padEnd(6)} ${summary(a)}`);
    if (STATE[a.state] !== "idle") { console.log(`      not Idle: finish --phase pre first`); continue; }
    try {
      const before = keeperCmd("venues", "check", "--vault", v.symbol);
      process.stdout.write(before.split("\n").map((l) => `      ${l}`).join("\n") + "\n");
      keeperCmd("venues", "setup", "--vault", v.symbol);
      const after = keeperCmd("venues", "sync-collateral", "--vault", v.symbol);
      if (!dryRun) {
        process.stdout.write(after.split("\n").map((l) => `      ${l}`).join("\n") + "\n");
        if (/synced: yes/.test(after) && /obligation .*: exists/.test(after) && /\(onboarded\)/.test(after)) synced++;
      }
    } catch (e) {
      console.log(`      failed: ${String((e as Error).message ?? e).slice(0, 300)}`);
    }
  }
  if (!dryRun) console.log(`\n${synced}/${vaults.length} vaults have an obligation, an onboarded Phoenix trader and empty custody. Re-run until all do, then start the keeper with program_build = "real".`);
}

if (phase === "pre") await pre(); else await post();
