// Polls getSignaturesForAddress for the program and processes new transactions
// oldest-first from a cursor persisted to a JSON file.
import fs from "node:fs";
import { Connection, PublicKey } from "@solana/web3.js";
import { decodeLogs } from "../decode.js";
import type { Writer } from "../write.js";
import { fetchTx } from "./rpc.js";

interface Cursor {
  lastSignature: string | null;
}

export function readCursor(path: string): Cursor {
  try {
    return JSON.parse(fs.readFileSync(path, "utf8")) as Cursor;
  } catch {
    return { lastSignature: null };
  }
}

export function writeCursor(path: string, cursor: Cursor): void {
  const tmp = `${path}.tmp`;
  fs.writeFileSync(tmp, JSON.stringify(cursor));
  fs.renameSync(tmp, path);
}

/** One polling pass. Returns the number of transactions processed. */
export async function pollOnce(opts: {
  conn: Connection;
  programId: string;
  writer: Writer;
  cursorPath: string;
}): Promise<number> {
  const cursor = readCursor(opts.cursorPath);
  const program = new PublicKey(opts.programId);

  // Newest first from RPC; walk pages until we hit the cursor, then replay oldest-first.
  const collected: string[] = [];
  let before: string | undefined;
  for (let page = 0; page < 20; page += 1) {
    const sigs = await opts.conn.getSignaturesForAddress(
      program,
      { before, until: cursor.lastSignature ?? undefined, limit: 1000 },
      "confirmed",
    );
    if (sigs.length === 0) break;
    for (const s of sigs) if (!s.err) collected.push(s.signature);
    before = sigs[sigs.length - 1].signature;
    if (sigs.length < 1000) break;
  }
  if (collected.length === 0) return 0;

  collected.reverse();
  let processed = 0;
  for (const signature of collected) {
    const tx = await fetchTx(opts.conn, signature);
    if (tx) {
      await opts.writer.writeTransaction(tx, decodeLogs(tx.logs, opts.programId));
      processed += 1;
    }
    writeCursor(opts.cursorPath, { lastSignature: signature });
  }
  return processed;
}

export function startPolling(opts: Parameters<typeof pollOnce>[0] & { intervalMs: number }): NodeJS.Timeout {
  const tick = async () => {
    try {
      const n = await pollOnce(opts);
      if (n > 0) console.log(`[poll] processed ${n} tx`);
    } catch (err) {
      console.error("[poll] failed:", err);
    }
  };
  void tick();
  return setInterval(tick, opts.intervalMs);
}
