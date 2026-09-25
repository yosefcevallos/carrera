export interface Config {
  supabaseUrl: string;
  supabaseServiceRoleKey: string;
  programId: string;
  rpcUrl: string;
  heliusWebhookSecret: string;
  keeperUrl: string;
  source: "helius" | "poll";
  port: number;
  cursorPath: string;
  pollIntervalMs: number;
  keeperIntervalMs: number;
  phoenixHistory: boolean;
  phoenixApiUrl: string;
  phoenixHistoryHours: number;
  phoenixPollMs: number;
}

function req(name: string): string {
  const v = process.env[name];
  if (!v) throw new Error(`Missing env ${name}`);
  return v;
}

export function loadConfig(env = process.env): Config {
  const source = env.SOURCE === "helius" ? "helius" : "poll";
  return {
    supabaseUrl: req("SUPABASE_URL"),
    supabaseServiceRoleKey: req("SUPABASE_SERVICE_ROLE_KEY"),
    programId: req("PROGRAM_ID"),
    rpcUrl: env.RPC_URL ?? "http://127.0.0.1:8899",
    heliusWebhookSecret: env.HELIUS_WEBHOOK_SECRET ?? "",
    keeperUrl: env.KEEPER_URL ?? "",
    source,
    port: Number(env.PORT ?? 8790),
    cursorPath: env.CURSOR_PATH ?? ".cursor.json",
    pollIntervalMs: Number(env.POLL_INTERVAL_MS ?? 15_000),
    keeperIntervalMs: Number(env.KEEPER_INTERVAL_MS ?? 60_000),
    phoenixHistory: (env.PHOENIX_HISTORY ?? "1") !== "0",
    phoenixApiUrl: env.PHOENIX_API_URL ?? "https://perp-api.phoenix.trade",
    phoenixHistoryHours: Number(env.PHOENIX_HISTORY_HOURS ?? 168),
    phoenixPollMs: Number(env.PHOENIX_POLL_SECS ?? 3600) * 1000,
  };
}
