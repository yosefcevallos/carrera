import { Connection } from "@solana/web3.js";
import { loadConfig } from "./config.js";
import { Writer } from "./write.js";
import { startHeliusServer } from "./sources/helius.js";
import { startPolling } from "./sources/poll.js";
import { startKeeperPoller } from "./sources/keeper.js";
import { startPhoenixFunding } from "./sources/phoenix.js";

async function main() {
  const cfg = loadConfig();
  const conn = new Connection(cfg.rpcUrl, "confirmed");
  const writer = await Writer.connect(cfg.supabaseUrl, cfg.supabaseServiceRoleKey);
  console.log(`[indexer] program ${cfg.programId}, ${writer.vaults.size} vaults known, source=${cfg.source}`);

  if (cfg.source === "helius") {
    startHeliusServer({ port: cfg.port, secret: cfg.heliusWebhookSecret, programId: cfg.programId, conn, writer });
  } else {
    startPolling({ conn, programId: cfg.programId, writer, cursorPath: cfg.cursorPath, intervalMs: cfg.pollIntervalMs });
  }
  if (cfg.keeperUrl) startKeeperPoller(writer.db, cfg.keeperUrl, cfg.keeperIntervalMs);
  if (cfg.phoenixHistory) {
    startPhoenixFunding({
      db: writer.db,
      symbols: [...new Set(writer.vaults.values())].sort(),
      apiUrl: cfg.phoenixApiUrl,
      historyHours: cfg.phoenixHistoryHours,
      intervalMs: cfg.phoenixPollMs,
    });
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
