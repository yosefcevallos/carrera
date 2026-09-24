// Initialise the registry and the nine vaults on the cluster in ANCHOR_PROVIDER_URL.
// Idempotent: skips anything that already exists. Prints PDAs as JSON at the end.
//
//   ANCHOR_PROVIDER_URL=<rpc> ANCHOR_WALLET=~/.config/solana/id.json \
//   KEEPER_PUBKEY=<pubkey> CAP_USD=25000 pnpm init
import anchor from "@coral-xyz/anchor";
import { PublicKey, SystemProgram, SYSVAR_RENT_PUBKEY } from "@solana/web3.js";
import { TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID } from "@solana/spl-token";
import { readFileSync } from "node:fs";
import { vaultParams, TIER_INDEX, type TierName } from "./params.ts";

const idl = JSON.parse(readFileSync(new URL("../program/target/idl/carrera_overlay.json", import.meta.url), "utf8"));
const cfg = JSON.parse(readFileSync(process.env.VAULTS_JSON ?? new URL("./vaults.json", import.meta.url), "utf8"));

const provider = anchor.AnchorProvider.env();
anchor.setProvider(provider);
const program = new anchor.Program(idl, provider);
const admin = provider.wallet.publicKey;
const keeper = new PublicKey(process.env.KEEPER_PUBKEY!);
const guardian = process.env.GUARDIAN_PUBKEY ? new PublicKey(process.env.GUARDIAN_PUBKEY) : admin;
const capUsd = Number(process.env.CAP_USD ?? "25000");
const usdcMint = new PublicKey(cfg.usdc);

const [registry] = PublicKey.findProgramAddressSync([Buffer.from("registry")], program.programId);

async function priceUsd(mint: string): Promise<number> {
  const r = await fetch(`https://lite-api.jup.ag/price/v3?ids=${mint}`);
  const j = r.ok ? ((await r.json()) as Record<string, { usdPrice: number } | undefined>) : {};
  const p = j[mint]?.usdPrice;
  if (p) return Number(p);
  if (process.env.FALLBACK_PRICE_USD) return Number(process.env.FALLBACK_PRICE_USD); // test mints on devnet
  throw new Error(`no Jupiter price for ${mint}`);
}

async function exists(pk: PublicKey) {
  return (await provider.connection.getAccountInfo(pk)) !== null;
}

const ixAccounts = (name: string): string[] =>
  (idl.instructions as { name: string; accounts: { name: string }[] }[]).find((i) => i.name === name)!.accounts.map((a) => a.name);

async function main() {
  console.log("cluster :", provider.connection.rpcEndpoint);
  console.log("program :", program.programId.toBase58());
  console.log("admin   :", admin.toBase58());
  console.log("keeper  :", keeper.toBase58());

  if (await exists(registry)) {
    console.log("registry exists, skipping init_registry");
  } else {
    const sig = await program.methods
      .initRegistry(guardian, [keeper])
      .accounts({ admin, usdcMint, systemProgram: SystemProgram.programId })
      .rpc();
    console.log("init_registry", sig);
  }

  const takesStockTokenProgram = ixAccounts("init_vault").includes("stock_token_program");
  const out: Record<string, unknown>[] = [];

  for (const v of cfg.vaults as { symbol: string; tier: TierName; mint: string }[]) {
    if (!v.mint) { console.log(`${v.symbol}: mint missing, skipped`); continue; }
    const xstockMint = new PublicKey(v.mint);
    const [vault] = PublicKey.findProgramAddressSync([Buffer.from("vault"), xstockMint.toBuffer()], program.programId);
    const [shareMint] = PublicKey.findProgramAddressSync([Buffer.from("shares"), vault.toBuffer()], program.programId);
    const mintInfo = await provider.connection.getAccountInfo(xstockMint);
    if (!mintInfo) throw new Error(`${v.symbol}: mint ${v.mint} not found on this cluster`);
    const stockTokenProgram = mintInfo.owner.equals(TOKEN_2022_PROGRAM_ID) ? TOKEN_2022_PROGRAM_ID : TOKEN_PROGRAM_ID;
    const decimals = mintInfo.data[44];

    const px = await priceUsd(v.mint);
    const depositCap = BigInt(Math.floor((capUsd / px) * 10 ** decimals));
    const basisCap = BigInt(capUsd) * 1_000_000n;
    const params = vaultParams(v.tier, depositCap, basisCap);

    if (await exists(vault)) {
      console.log(`${v.symbol}: vault exists (${vault.toBase58()}), skipping init_vault`);
    } else {
      const accounts: Record<string, PublicKey> = {
        admin, xstockMint, usdcMint,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
        rent: SYSVAR_RENT_PUBKEY,
      };
      if (takesStockTokenProgram) accounts.stockTokenProgram = stockTokenProgram;
      const sig = await program.methods.initVault(TIER_INDEX[v.tier], params).accounts(accounts).rpc();
      console.log(`${v.symbol}: init_vault ${sig}  cap ${capUsd} USD = ${depositCap} base units at $${px.toFixed(2)}`);
    }
    out.push({
      symbol: v.symbol, tier: TIER_INDEX[v.tier], xstock_mint: v.mint,
      vault_pubkey: vault.toBase58(), share_mint: shareMint.toBase58(),
      token_program: stockTokenProgram.toBase58(), decimals, price_usd: px, deposit_cap_base: depositCap.toString(),
    });
  }
  console.log("\nVAULTS_JSON " + JSON.stringify({ program_id: program.programId.toBase58(), registry: registry.toBase58(), vaults: out }));
}

main().catch((e) => { console.error(e); process.exit(1); });
