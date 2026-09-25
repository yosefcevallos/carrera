// Settle pending exits now (keeper-signed): for each vault with pending exit shares, make sure the
// market flag is open, unwind a Basis position for exit demand, close the current epoch and settle it.
// Resumable and retrying, like wind-now.ts. Empty vaults and vaults without exits are skipped.
//   ANCHOR_PROVIDER_URL=<rpc> ANCHOR_WALLET=~/.config/carrera/keeper.json tsx settle-now.ts
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
const TOKEN = new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const TOKEN_2022 = new PublicKey("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
const EXIT_DEMAND = 1;
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function priceE6(mint: string): Promise<number> {
  const r = await fetch(`https://lite-api.jup.ag/price/v3?ids=${mint}`);
  const j = r.ok ? ((await r.json()) as Record<string, { usdPrice: number } | undefined>) : {};
  const p = j[mint]?.usdPrice;
  if (!p) throw new Error(`no Jupiter price for ${mint}`);
  return Math.round(p * 1e6);
}

async function send(label: string, f: () => Promise<string>): Promise<void> {
  for (let i = 0; i < 4; i++) {
    try { await f(); return; } catch (e) {
      const msg = String((e as Error).message ?? e);
      if (i < 3 && /WrongState|WrongStep|MarketClosed|NavStale|EpochNotClosed/.test(msg)) { await sleep(2500); continue; }
      throw new Error(`${label}: ${msg}`);
    }
  }
}

for (const v of cfg.vaults as { symbol: string; mint: string }[]) {
  const mint = new PublicKey(v.mint);
  const [vault] = PublicKey.findProgramAddressSync([Buffer.from("vault"), mint.toBuffer()], program.programId);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const read = async () => (program.account as any).overlayVault.fetch(vault);
  let a = await read();
  if (a.pendingExitShares.toString() === "0") { console.log(`${v.symbol.padEnd(6)} no pending exits`); continue; }
  try {
    const px = await priceE6(v.mint);
    await send("refresh_nav", () => program.methods.refreshNav(new BN(px)).accounts({ signer: keeper, vault, oracle: SystemProgram.programId }).rpc());
    if (!a.marketOpen) await send("set_market_open", () => program.methods.setMarketOpen(true).accounts({ keeper, vault }).rpc());

    // Release the position so the stock can leave: Basis → unwind for exit demand → Parked → repay → Idle.
    a = await read();
    if (STATE[a.state] === "basis") {
      await send("unwind_start", () => program.methods.unwindStart(EXIT_DEMAND).accounts({ keeper, vault }).rpc());
      a = await read();
    }
    if (STATE[a.state] === "unwinding") {
      for (let n = Number(a.step) + 1; n <= 3; n++) await send(`unwind_step(${n})`, () => program.methods.unwindStep(n).accounts({ keeper, vault }).rpc());
      await send("unwind_commit", () => program.methods.unwindCommit().accounts({ keeper, vault }).rpc());
      a = await read();
    }
    if (STATE[a.state] === "parked") {
      await send("repay", () => program.methods.repay().accounts({ keeper, vault }).rpc());
      a = await read();
    }
    // Residual debt dust after a mock unwind: credit the shortfall on the parked leg (mock build only) and repay from Idle.
    if (STATE[a.state] === "idle" && a.debtUsdc.toString() !== "0") {
      const dust = new BN(a.debtUsdc.toString());
      await send("mock_accrue", () => program.methods.mockAccrue(dust, 0).accounts({ keeper, vault }).rpc());
      await send("repay", () => program.methods.repay().accounts({ keeper, vault }).rpc());
      a = await read();
      console.log(`${v.symbol.padEnd(6)} cleared ${dust.toString()} base units of debt dust`);
    }

    const epochId = new BN(a.epochId.toString());
    const [exitEpoch] = PublicKey.findProgramAddressSync([Buffer.from("epoch"), vault.toBuffer(), Buffer.from(epochId.toArrayLike(Buffer, "le", 8))], program.programId);
    await send("close_epoch", () => program.methods.closeEpoch().accounts({ keeper, vault, exitEpoch, systemProgram: SystemProgram.programId }).rpc());
    await send("settle_epoch", () => program.methods.settleEpoch().accounts({ keeper, vault, exitEpoch, xstockMint: mint, tokenProgram: TOKEN, stockTokenProgram: TOKEN_2022 }).rpc());
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const ep = await (program.account as any).exitEpoch.fetch(exitEpoch);
    a = await read();
    console.log(`${v.symbol.padEnd(6)} epoch ${epochId.toString()} settled: ${ep.sharesTotal.toString()} shares → ${(Number(ep.stockPaid) / 1e8).toFixed(6)} ${v.symbol}x + ${(Number(ep.usdcPaid) / 1e6).toFixed(2)} USDC; vault now ${STATE[a.state]}, next epoch ${a.epochId.toString()}`);
  } catch (e) {
    const msg = String((e as Error).message ?? e);
    const m = msg.match(/Error Code: (\w+)/);
    a = await read();
    console.log(`${v.symbol.padEnd(6)} not settled (state ${STATE[a.state]}, step ${a.step}): ${m ? m[1] : msg.slice(0, 200)}`);
  }
}
