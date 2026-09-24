// Runs only against a live stack: SUPABASE_LIVE=1 NEXT_PUBLIC_SUPABASE_URL=... NEXT_PUBLIC_SUPABASE_PUBLISHABLE_KEY=... pnpm test
import { describe, expect, it } from "vitest";
import { TICKERS } from "@/constants/vaults";
import { fetchHistory } from "@/lib/history";
import { zeroed } from "@/lib/zeroed";

describe.skipIf(!process.env.SUPABASE_LIVE)("fetchHistory against the local Supabase stack", () => {
  it("reads nav_samples and the views, every ticker present", async () => {
    const prices = { ...zeroed(TICKERS), TSLA: 412 };
    const snap = await fetchHistory(prices);
    for (const t of TICKERS) expect(snap.vaults[t]).toBeDefined();
    console.log("TSLA history:", JSON.stringify(snap.vaults.TSLA.sharePriceHistory));
    console.log("TSLA trailing:", JSON.stringify(snap.vaults.TSLA.trailing), "ageDays", snap.vaults.TSLA.ageDays);
    console.log("protocol:", snap.usdcPaid24h, snap.depositors);
    expect(snap.vaults.TSLA.sharePriceHistory.length).toBeGreaterThanOrEqual(2);
  });
});
