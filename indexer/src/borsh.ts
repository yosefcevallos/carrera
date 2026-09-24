// Minimal Borsh reader for the fixed-width fields Anchor events use.
import { PublicKey } from "@solana/web3.js";

export class Reader {
  private off = 0;
  constructor(private readonly buf: Buffer) {}

  get remaining(): number {
    return this.buf.length - this.off;
  }

  u8(): number {
    const v = this.buf.readUInt8(this.off);
    this.off += 1;
    return v;
  }

  bool(): boolean {
    return this.u8() !== 0;
  }

  u32(): number {
    const v = this.buf.readUInt32LE(this.off);
    this.off += 4;
    return v;
  }

  u64(): bigint {
    const v = this.buf.readBigUInt64LE(this.off);
    this.off += 8;
    return v;
  }

  i64(): bigint {
    const v = this.buf.readBigInt64LE(this.off);
    this.off += 8;
    return v;
  }

  pubkey(): string {
    const v = new PublicKey(this.buf.subarray(this.off, this.off + 32)).toBase58();
    this.off += 32;
    return v;
  }
}
