// HTTP receiver for Helius webhooks (raw or enhanced) registered on the program id.
// Raw payloads carry logMessages; enhanced ones do not, so those are refetched by signature.
import http from "node:http";
import { Connection } from "@solana/web3.js";
import { decodeLogs } from "../decode.js";
import type { Writer } from "../write.js";
import { fetchTx } from "./rpc.js";

interface HeliusItem {
  signature?: string;
  slot?: number;
  timestamp?: number;
  transaction?: { signatures?: string[] };
  meta?: { logMessages?: string[]; err?: unknown };
}

export function extractLogs(item: HeliusItem): { signature: string; slot: number; blockTime: Date; logs: string[] } | null {
  const signature = item.signature ?? item.transaction?.signatures?.[0];
  if (!signature) return null;
  if (item.meta?.err) return null;
  const logs = item.meta?.logMessages;
  if (!logs) return null;
  return {
    signature,
    slot: item.slot ?? 0,
    blockTime: new Date((item.timestamp ?? Math.floor(Date.now() / 1000)) * 1000),
    logs,
  };
}

export function startHeliusServer(opts: {
  port: number;
  secret: string;
  programId: string;
  conn: Connection;
  writer: Writer;
}): http.Server {
  const server = http.createServer(async (req, res) => {
    if (req.method === "GET" && req.url === "/healthz") {
      res.writeHead(200).end("ok");
      return;
    }
    if (req.method !== "POST" || req.url !== "/webhook/helius") {
      res.writeHead(404).end();
      return;
    }
    if (opts.secret && req.headers.authorization !== opts.secret) {
      res.writeHead(401).end();
      return;
    }
    let body = "";
    for await (const chunk of req) body += chunk;
    let items: HeliusItem[];
    try {
      const parsed = JSON.parse(body);
      items = Array.isArray(parsed) ? parsed : [parsed];
    } catch {
      res.writeHead(400).end("bad json");
      return;
    }
    // Acknowledge fast; Helius retries on non-2xx.
    res.writeHead(200).end("ok");

    for (const item of items) {
      try {
        let tx = extractLogs(item);
        if (!tx) {
          const sig = item.signature ?? item.transaction?.signatures?.[0];
          if (!sig) continue;
          const fetched = await fetchTx(opts.conn, sig);
          if (!fetched) continue;
          tx = fetched;
        }
        const events = decodeLogs(tx.logs, opts.programId);
        await opts.writer.writeTransaction(tx, events);
      } catch (err) {
        console.error("[helius] failed:", err);
      }
    }
  });
  server.listen(opts.port, () => console.log(`[helius] listening on :${opts.port}`));
  return server;
}
