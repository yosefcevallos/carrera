import { describe, expect, it } from "vitest";
import { Keypair } from "@solana/web3.js";
import { decodeEventBytes, decodeLogs, eventDiscriminator, payloadToJson } from "../src/decode.js";

const PROGRAM = "GH45ANLzg1t6rNnaoNqE39rN1rKGXFQXPZnNxvbhUmYw";

function u64(v: bigint) { const b = Buffer.alloc(8); b.writeBigUInt64LE(v); return b; }
function i64(v: bigint) { const b = Buffer.alloc(8); b.writeBigInt64LE(v); return b; }
function u32(v: number) { const b = Buffer.alloc(4); b.writeUInt32LE(v); return b; }
const u8 = (v: number) => Buffer.from([v]);

describe("discriminators match the built IDL", () => {
  // Known vectors copied from program/target/idl/carrera_overlay.json
  it.each([
    ["Deposited", "6f8d1a2da1236439"],
    ["RuleEvaluated", "ad31fed90b140ab3"],
    ["NavRefreshed", "82ea401109eea128"],
    ["StateChanged", "009637e02b1ad6c0"],
    ["FundingRecorded", "7b54d1f3b3a8c24d"],
    ["Redeemed", "0e1db7471fa56b26"],
    ["Paused", "acf805fd31ffffe8"],
  ])("%s", (name, hex) => {
    expect(eventDiscriminator(name).toString("hex")).toBe(hex);
  });
});

describe("decodeEventBytes", () => {
  const vault = Keypair.generate().publicKey;
  const user = Keypair.generate().publicKey;

  it("RuleEvaluated", () => {
    const bytes = Buffer.concat([
      eventDiscriminator("RuleEvaluated"), vault.toBuffer(),
      i64(31200n), u32(480), u32(590), i64(19070n), u8(1),
    ]);
    const out = decodeEventBytes(bytes);
    expect(out?.name).toBe("RuleEvaluated");
    // Mock-build event: no D8 tail.
    expect(out?.payload).toEqual({ vault: vault.toBase58(), f_avg_bps: 31200n, parked_apy_bps: 480, r_bps: 590, hurdle_bps: 19070n, decision: 1, f_3h_bps: null, be_bps: null });
    // Real-legs event (D8): the 3h average and break-even follow.
    const d8 = Buffer.concat([bytes, i64(33000n), i64(805n)]);
    expect(decodeEventBytes(d8)?.payload).toEqual({ vault: vault.toBase58(), f_avg_bps: 31200n, parked_apy_bps: 480, r_bps: 590, hurdle_bps: 19070n, decision: 1, f_3h_bps: 33000n, be_bps: 805n });
  });

  it("Redeemed", () => {
    const bytes = Buffer.concat([
      eventDiscriminator("Redeemed"), vault.toBuffer(), user.toBuffer(), u64(3n), u64(10n), u64(10n), u64(12_910_000n),
    ]);
    expect(decodeEventBytes(bytes)?.payload).toEqual({ vault: vault.toBase58(), user: user.toBase58(), nonce: 3n, shares: 10n, stock: 10n, usdc: 12_910_000n });
  });

  it("NavRefreshed carries price_e6 and the optional debt_dust_usdc", () => {
    const bytes = Buffer.concat([eventDiscriminator("NavRefreshed"), vault.toBuffer(), u64(4_120_000_000n), u64(1_020_000n), u64(412_300_000n), u64(2_944n)]);
    expect(decodeEventBytes(bytes)?.payload).toEqual({ vault: vault.toBase58(), nav_usd_e6: 4_120_000_000n, share_price_stock_e6: 1_020_000n, price_e6: 412_300_000n, debt_dust_usdc: 2_944n });
  });

  it("negative funding sample", () => {
    const bytes = Buffer.concat([eventDiscriminator("FundingRecorded"), vault.toBuffer(), i64(-4_000_000n), i64(-35040n), u8(3)]);
    expect(decodeEventBytes(bytes)?.payload).toEqual({ vault: vault.toBase58(), rate_bps_e6_hourly: -4_000_000n, f_avg_bps: -35040n, samples: 3 });
  });

  it("unknown discriminator and truncated payloads return null", () => {
    expect(decodeEventBytes(Buffer.from("0000000000000000", "hex"))).toBeNull();
    expect(decodeEventBytes(Buffer.concat([eventDiscriminator("Deposited"), vault.toBuffer()]))).toBeNull();
  });

  it("payloadToJson stringifies bigints", () => {
    expect(payloadToJson({ a: 1n, b: 2, c: "x" })).toEqual({ a: "1", b: 2, c: "x" });
  });
});

describe("decodeLogs", () => {
  const vault = Keypair.generate().publicKey;
  const ev = Buffer.concat([eventDiscriminator("StateChanged"), vault.toBuffer(), u8(1), u8(2), u8(0)]).toString("base64");
  const other = "OtherProgram1111111111111111111111111111111";

  it("only decodes data emitted while our program is on the stack, and tracks ix index", () => {
    const logs = [
      `Program ${other} invoke [1]`,
      `Program data: ${ev}`,                // not ours
      `Program ${other} success`,
      `Program ${PROGRAM} invoke [1]`,
      `Program log: Instruction: WindStart`,
      `Program ${other} invoke [2]`,        // CPI from ours
      `Program data: ${ev}`,                // inner program's data, skipped
      `Program ${other} success`,
      `Program data: ${ev}`,                // ours
      `Program ${PROGRAM} success`,
      `Program ${PROGRAM} invoke [1]`,
      `Program data: ${ev}`,                // ours, second instruction
      `Program ${PROGRAM} success`,
    ];
    const out = decodeLogs(logs, PROGRAM);
    expect(out).toHaveLength(2);
    expect(out[0]).toMatchObject({ name: "StateChanged", ixIndex: 1, logIndex: 0, payload: { from: 1, to: 2, step: 0 } });
    expect(out[1]).toMatchObject({ ixIndex: 2, logIndex: 1 });
  });
});
