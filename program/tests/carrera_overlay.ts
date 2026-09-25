import * as anchor from "@coral-xyz/anchor";
import { BN, Program } from "@coral-xyz/anchor";
import { Keypair, PublicKey, SystemProgram, SYSVAR_RENT_PUBKEY, Transaction } from "@solana/web3.js";
import {
  createMint,
  createAssociatedTokenAccount,
  createInitializeMintInstruction,
  createInitializePausableConfigInstruction,
  createInitializePermanentDelegateInstruction,
  createInitializeTransferHookInstruction,
  ExtensionType,
  getAccount,
  getMintLen,
  mintTo,
  TOKEN_2022_PROGRAM_ID,
  TOKEN_PROGRAM_ID,
} from "@solana/spl-token";
import { assert } from "chai";
import { CarreraOverlay } from "../target/types/carrera_overlay";

/** Empty `venue_data`: mock builds take no venue accounts. */
const NO_VENUE = Buffer.alloc(0);

// Built with `--features mock-venues`: venue legs are simulated from the cached price.
// The stock mint is Token-2022 with the extensions the live xStocks carry that touch
// transfers (transfer hook with no program set, permanent delegate, pausable).

const ONE_STOCK = 100_000_000n; // 8 decimals
const PRICE_E6 = new BN(412_000_000); // $412.00
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const u64le = (n: number | bigint) => {
  const b = Buffer.alloc(8);
  b.writeBigUInt64LE(BigInt(n));
  return b;
};
// 35% annualised → hourly bps × 1e6
const FUNDING_35PCT = new BN(Math.floor((3500 * 1_000_000) / 8760));

describe("carrera_overlay", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = anchor.workspace.carreraOverlay as Program<CarreraOverlay>;
  const wallet = (provider.wallet as anchor.Wallet).payer;
  const admin = wallet.publicKey; // admin, keeper and user are all the test wallet
  const guardian = Keypair.generate().publicKey; // separate so keeper-only rule checks are exercised

  let xstockMint: PublicKey;
  let usdcMint: PublicKey;
  let userStock: PublicKey;
  let userUsdc: PublicKey;
  let userShares: PublicKey;
  let registry: PublicKey;
  let vault: PublicKey;
  let shareMint: PublicKey;
  let stockCustody: PublicKey;
  let usdcBuffer: PublicKey;
  let redeemStock: PublicKey;
  let redeemUsdc: PublicKey;
  let escrowShares: PublicKey;
  const unchecked = Keypair.generate().publicKey; // placeholder for oracle / hawkeye / reserve accounts

  const params = {
    ltvBps: 3000,
    minMarginBps: 1200,
    liqLtvBps: 6500,
    emergencyLtvBps: 6000,
    enterMarginBps: 200,
    exitMarginBps: 100,
    carryGuardMarginBps: 50,
    minEnterFundingBps: 450,
    expectedHoldHours: 720,
    roundtripCostBps: 60,
    fundingWindow: 24,
    maxSwapSlippageBps: 50,
    maxPerpSlippageBps: 30,
    maxIndexDevBps: 50,
    hedgeTolBps: 50,
    sizeBandBps: 300,
    rebalanceLtvBandBps: 800,
    rebalanceMarginBandBps: 500,
    maxNavAgeSlots: new BN(150),
    epochLenSecs: new BN(1),
    perfFeeBps: 1500,
    exitFeeBps: 10,
    depositCapStock: new BN(0),
    basisCapUsdc: new BN(0),
  };

  const keeperCtx = () => ({ keeper: admin, registry, vault });
  const refreshNav = () =>
    program.methods.refreshNav(PRICE_E6).accountsPartial({ signer: admin, registry, vault, oracle: unchecked }).rpc();
  const setRates = (borrow: number, supply: number) =>
    program.methods
      .recordKaminoRates(borrow, supply)
      .accountsPartial({ signer: admin, registry, kaminoReserve: unchecked })
      .rpc();
  const fetchVault = () => program.account.overlayVault.fetch(vault);
  const expectError = async (p: Promise<unknown>, code: string) => {
    try {
      await p;
    } catch (e: any) {
      const msg = e.error?.errorCode?.code ?? e.toString();
      assert.include(String(msg), code);
      return;
    }
    assert.fail(`expected error ${code}`);
  };

  async function createXStockMint2022(): Promise<PublicKey> {
    const mint = Keypair.generate();
    const extensions = [ExtensionType.TransferHook, ExtensionType.PermanentDelegate, ExtensionType.PausableConfig];
    const space = getMintLen(extensions);
    const lamports = await provider.connection.getMinimumBalanceForRentExemption(space);
    const tx = new Transaction().add(
      SystemProgram.createAccount({
        fromPubkey: admin, newAccountPubkey: mint.publicKey, space, lamports, programId: TOKEN_2022_PROGRAM_ID,
      }),
      // PublicKey.default = no hook program, as on the live mints.
      createInitializeTransferHookInstruction(mint.publicKey, admin, PublicKey.default, TOKEN_2022_PROGRAM_ID),
      createInitializePermanentDelegateInstruction(mint.publicKey, admin, TOKEN_2022_PROGRAM_ID),
      createInitializePausableConfigInstruction(mint.publicKey, admin, TOKEN_2022_PROGRAM_ID),
      createInitializeMintInstruction(mint.publicKey, 8, admin, admin, TOKEN_2022_PROGRAM_ID),
    );
    await provider.sendAndConfirm(tx, [mint]);
    return mint.publicKey;
  }

  before(async () => {
    xstockMint = await createXStockMint2022();
    usdcMint = await createMint(provider.connection, wallet, admin, null, 6);
    userStock = await createAssociatedTokenAccount(
      provider.connection, wallet, xstockMint, admin, undefined, TOKEN_2022_PROGRAM_ID,
    );
    userUsdc = await createAssociatedTokenAccount(provider.connection, wallet, usdcMint, admin);
    await mintTo(
      provider.connection, wallet, xstockMint, userStock, wallet, 100n * ONE_STOCK, [], undefined, TOKEN_2022_PROGRAM_ID,
    );

    [registry] = PublicKey.findProgramAddressSync([Buffer.from("registry")], program.programId);
    [vault] = PublicKey.findProgramAddressSync([Buffer.from("vault"), xstockMint.toBuffer()], program.programId);
    [shareMint] = PublicKey.findProgramAddressSync([Buffer.from("shares"), vault.toBuffer()], program.programId);
    [stockCustody] = PublicKey.findProgramAddressSync([Buffer.from("stock"), vault.toBuffer()], program.programId);
    [usdcBuffer] = PublicKey.findProgramAddressSync([Buffer.from("usdc"), vault.toBuffer()], program.programId);
    [redeemStock] = PublicKey.findProgramAddressSync([Buffer.from("redeem_stock"), vault.toBuffer()], program.programId);
    [redeemUsdc] = PublicKey.findProgramAddressSync([Buffer.from("redeem_usdc"), vault.toBuffer()], program.programId);
    [escrowShares] = PublicKey.findProgramAddressSync([Buffer.from("escrow"), vault.toBuffer()], program.programId);
  });

  it("initialises the registry and a vault", async () => {
    await program.methods
      .initRegistry(guardian, [admin])
      .accountsPartial({ admin, registry, usdcMint, systemProgram: SystemProgram.programId })
      .rpc();
    await program.methods
      .initVault(1, params)
      .accountsPartial({
        admin,
        registry,
        vault,
        xstockMint,
        shareMint,
        stockCustody,
        usdcBuffer,
        redeemStock,
        redeemUsdc,
        usdcMint,
        tokenProgram: TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
        rent: SYSVAR_RENT_PUBKEY,
        stockTokenProgram: TOKEN_2022_PROGRAM_ID,
      })
      .rpc();
    const r = await program.account.registry.fetch(registry);
    assert.equal(r.keeperCount, 1);
    const v = await fetchVault();
    assert.equal(v.state, 0);
    assert.equal(v.stockDecimals, 8);
    assert.equal(v.sharePriceStockE6.toNumber(), 1_000_000);
    userShares = await createAssociatedTokenAccount(provider.connection, wallet, shareMint, admin);
    await program.methods.setMarketOpen(true).accountsPartial(keeperCtx()).rpc();
  });

  it("deposit mints shares 1:1 at genesis", async () => {
    await expectError(
      program.methods
        .deposit(new BN(10n * ONE_STOCK), new BN(0))
        .accountsPartial({ user: admin, registry, vault, shareMint, xstockMint, stockCustody, userStock, userShares, tokenProgram: TOKEN_PROGRAM_ID, stockTokenProgram: TOKEN_2022_PROGRAM_ID })
        .rpc(),
      "NavStale",
    );
    await refreshNav();
    await program.methods
      .deposit(new BN(10n * ONE_STOCK), new BN(0))
      .accountsPartial({ user: admin, registry, vault, shareMint, xstockMint, stockCustody, userStock, userShares, tokenProgram: TOKEN_PROGRAM_ID, stockTokenProgram: TOKEN_2022_PROGRAM_ID })
      .rpc();
    const v = await fetchVault();
    assert.equal(BigInt(v.totalShares.toString()), 10n * ONE_STOCK);
    assert.equal(BigInt(v.collateralQty.toString()), 10n * ONE_STOCK);
    assert.equal(v.navUsdE6.toNumber(), 4_120_000_000);
    assert.equal(v.sharePriceStockE6.toNumber(), 1_000_000);
    const shares = await getAccount(provider.connection, userShares);
    assert.equal(shares.amount, 10n * ONE_STOCK);
    const custody = await getAccount(provider.connection, stockCustody, undefined, TOKEN_2022_PROGRAM_ID);
    assert.equal(custody.amount, 10n * ONE_STOCK);
    assert.isTrue((await provider.connection.getAccountInfo(stockCustody))!.owner.equals(TOKEN_2022_PROGRAM_ID));
  });

  it("park is refused when carry is negative (supply 4.8% < borrow 5.9%)", async () => {
    await setRates(590, 480);
    await expectError(program.methods.park(NO_VENUE).accountsPartial(keeperCtx()).rpc(), "RuleNotSatisfied");
    const v = await fetchVault();
    assert.equal(v.lastRule.decision, 0);
  });

  it("park → guard → repay when supply drops below borrow", async () => {
    await setRates(590, 650);
    await program.methods.park(NO_VENUE).accountsPartial(keeperCtx()).rpc();
    let v = await fetchVault();
    assert.equal(v.state, 1);
    assert.equal(v.debtUsdc.toNumber(), 1_236_000_000); // 30% of $4120
    assert.equal(v.parkedUsdc.toNumber(), 1_236_000_000);
    assert.equal(v.navUsdE6.toNumber(), 4_120_000_000); // loan is NAV-neutral
    assert.equal(v.lastRule.decision, 2);

    await expectError(program.methods.repay(NO_VENUE).accountsPartial(keeperCtx()).rpc(), "RuleNotSatisfied");
    await setRates(590, 480);
    await program.methods.repay(NO_VENUE).accountsPartial(keeperCtx()).rpc();
    v = await fetchVault();
    assert.equal(v.state, 0);
    assert.equal(v.debtUsdc.toNumber(), 0);
    assert.equal(v.parkedUsdc.toNumber(), 0);
    assert.equal(v.lastRule.decision, 3);
  });

  it("winds into Basis once 3 funding samples clear break-even (D8)", async () => {
    await setRates(590, 650);
    await program.methods.park(NO_VENUE).accountsPartial(keeperCtx()).rpc();
    // Not enough samples yet: two prints are not a 3h average.
    for (let i = 0; i < 2; i++) {
      await program.methods
        .recordFunding(FUNDING_35PCT)
        .accountsPartial({ signer: admin, registry, vault, hawkeyeView: unchecked })
        .rpc();
    }
    await expectError(program.methods.windStart(NO_VENUE).accountsPartial(keeperCtx()).rpc(), "RuleNotSatisfied");
    await program.methods
      .recordFunding(FUNDING_35PCT)
      .accountsPartial({ signer: admin, registry, vault, hawkeyeView: unchecked })
      .rpc();
    let v = await fetchVault();
    assert.equal(v.fundingSamples, 3);

    await program.methods.windStart(NO_VENUE).accountsPartial(keeperCtx()).rpc();
    v = await fetchVault();
    assert.equal(v.state, 2);
    assert.equal(v.lastRule.decision, 1);
    assert.equal(v.lastRule.hurdleBps.toNumber(), 767); // be = r + L·r = 590 + 177
    assert.approximately(v.lastRule.fAvgBps.toNumber(), 3500, 1);

    await expectError(program.methods.windStep(2, NO_VENUE).accountsPartial(keeperCtx()).rpc(), "WrongStep");
    await program.methods.windStep(1, NO_VENUE).accountsPartial(keeperCtx()).rpc();
    v = await fetchVault();
    assert.equal(BigInt(v.basisSpotQty.toString()), 3n * ONE_STOCK); // $1236 / $412
    assert.equal(BigInt(v.collateralQty.toString()), 13n * ONE_STOCK);
    assert.equal(v.parkedUsdc.toNumber(), 0);

    await program.methods.windStep(2, NO_VENUE).accountsPartial(keeperCtx()).rpc();
    v = await fetchVault();
    assert.equal(v.debtBUsdc.toNumber(), 370_800_000); // 30% of D
    assert.equal(v.phoenixEquityUsdc.toNumber(), 370_800_000);

    await program.methods.windStep(3, NO_VENUE).accountsPartial(keeperCtx()).rpc();
    await program.methods.windCommit(NO_VENUE).accountsPartial(keeperCtx()).rpc();
    v = await fetchVault();
    assert.equal(v.state, 3);
    assert.equal(v.step, 0);
    assert.equal(v.phoenixShortQty.toString(), v.basisSpotQty.toString());
    assert.equal(v.navUsdE6.toNumber(), 4_120_000_000); // hedged leg is NAV-neutral
  });

  it("request_exit → unwind for exit demand → close, settle, redeem pays stock + USDC", async () => {
    const nonce = new BN(1);
    const [exitRequest] = PublicKey.findProgramAddressSync(
      [Buffer.from("exit"), vault.toBuffer(), admin.toBuffer(), u64le(1)],
      program.programId,
    );
    const [exitEpoch] = PublicKey.findProgramAddressSync(
      [Buffer.from("epoch"), vault.toBuffer(), u64le(0)],
      program.programId,
    );
    await program.methods
      .requestExit(new BN(5n * ONE_STOCK), nonce)
      .accountsPartial({
        user: admin, registry, vault, exitRequest, exitEpoch, shareMint, userShares, escrowShares,
        tokenProgram: TOKEN_PROGRAM_ID, systemProgram: SystemProgram.programId,
      })
      .rpc();
    let v = await fetchVault();
    assert.equal(BigInt(v.pendingExitShares.toString()), 5n * ONE_STOCK);

    // Basis → Parked because exits are pending, then earn some USDC and go Idle via the guard.
    await program.methods.unwindStart(1).accountsPartial(keeperCtx()).rpc();
    for (const n of [1, 2, 3]) await program.methods.unwindStep(n, NO_VENUE).accountsPartial(keeperCtx()).rpc();
    await program.methods.unwindCommit(NO_VENUE).accountsPartial(keeperCtx()).rpc();
    v = await fetchVault();
    assert.equal(v.state, 1);
    assert.equal(v.debtBUsdc.toNumber(), 0);
    assert.equal(BigInt(v.basisSpotQty.toString()), 0n);
    assert.equal(v.parkedUsdc.toNumber(), 1_236_000_000);

    // Simulate $41.20 of USDC earned (1% of NAV) and back it with real tokens in the buffer.
    await program.methods.mockAccrue(new BN(41_200_000), 0).accountsPartial(keeperCtx()).rpc();
    await mintTo(provider.connection, wallet, usdcMint, usdcBuffer, wallet, 41_200_000n);
    await setRates(590, 480);
    await program.methods.repay(NO_VENUE).accountsPartial(keeperCtx()).rpc();
    v = await fetchVault();
    assert.equal(v.state, 0);
    assert.equal(v.parkedUsdc.toNumber(), 41_200_000);
    await refreshNav();
    v = await fetchVault();
    assert.equal(v.navUsdE6.toNumber(), 4_161_200_000);
    assert.equal(v.sharePriceStockE6.toNumber(), 1_010_000);

    await sleep(2500);
    await refreshNav();
    await program.methods
      .closeEpoch()
      .accountsPartial({ keeper: admin, registry, vault, exitEpoch, systemProgram: SystemProgram.programId })
      .rpc();
    let e = await program.account.exitEpoch.fetch(exitEpoch);
    assert.isTrue(e.closed);
    assert.equal(BigInt(e.stockOwed.toString()), 5n * ONE_STOCK);
    assert.equal(e.usdcOwed.toNumber(), 20_600_000); // half of the $41.20
    v = await fetchVault();
    assert.equal(v.epochId.toNumber(), 1);

    await refreshNav();
    await program.methods
      .settleEpoch(NO_VENUE)
      .accountsPartial({
        keeper: admin, registry, vault, exitEpoch, stockCustody, usdcBuffer, redeemStock, redeemUsdc,
        tokenProgram: TOKEN_PROGRAM_ID, xstockMint, stockTokenProgram: TOKEN_2022_PROGRAM_ID,
      })
      .rpc();
    e = await program.account.exitEpoch.fetch(exitEpoch);
    assert.isTrue(e.settled);
    v = await fetchVault();
    assert.equal(BigInt(v.totalShares.toString()), 5n * ONE_STOCK);
    assert.equal(BigInt(v.collateralQty.toString()), 5n * ONE_STOCK);
    assert.equal(v.pendingExitShares.toNumber(), 0);
    assert.equal(v.parkedUsdc.toNumber(), 20_600_000);
    assert.equal(v.sharePriceStockE6.toNumber(), 1_010_000); // remaining holders unaffected

    const stockBefore = (await getAccount(provider.connection, userStock, undefined, TOKEN_2022_PROGRAM_ID)).amount;
    await program.methods
      .redeem()
      .accountsPartial({
        user: admin, vault, exitRequest, exitEpoch, shareMint, escrowShares, redeemStock, redeemUsdc, userStock, userUsdc,
        tokenProgram: TOKEN_PROGRAM_ID, xstockMint, stockTokenProgram: TOKEN_2022_PROGRAM_ID,
      })
      .rpc();
    const stockAfter = (await getAccount(provider.connection, userStock, undefined, TOKEN_2022_PROGRAM_ID)).amount;
    const usdcAfter = (await getAccount(provider.connection, userUsdc)).amount;
    assert.equal(stockAfter - stockBefore, 5n * ONE_STOCK);
    assert.equal(usdcAfter, 20_600_000n);
    assert.isNull(await provider.connection.getAccountInfo(exitRequest)); // closed, rent returned
    assert.equal((await getAccount(provider.connection, escrowShares)).amount, 0n); // escrow burned
  });

  it("mock args are honoured only in mock builds; non-keepers are rejected", async () => {
    const stranger = Keypair.generate();
    await expectError(
      program.methods.setMarketOpen(false).accountsPartial({ keeper: stranger.publicKey, registry, vault }).signers([stranger]).rpc(),
      "Unauthorized",
    );
  });
});
