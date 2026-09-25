import { describe, expect, it } from "vitest";
import { TICKERS, type Ticker } from "@/constants/vaults";
import { poleSummary, rankVaults } from "@/lib/grid";
import type { VaultRecord } from "@/lib/types";
import { filled } from "@/lib/zeroed";
import { zeroVault } from "@/store/vault-store";

function world(overrides: Partial<Record<Ticker, Partial<VaultRecord>>>): Record<Ticker, VaultRecord> {
  const v = filled(TICKERS, zeroVault);
  for (const [t, o] of Object.entries(overrides)) v[t as Ticker] = { ...v[t as Ticker], ...o };
  return v;
}
// Basis vault at L=30%, r=589: APY = 0.3·f − 0.39·589
const basis = (fundingAvgBps: number, tvlUsd = 0): Partial<VaultRecord> => ({ vaultState: 3, ltvBps: 3000, borrowApyBps: 589, fundingAvgBps, tvlUsd });

describe("live grid ranking", () => {
  it("orders by current APY desc, breaks ties by TVL, and labels the mode", () => {
    const v = world({
      MSTR: basis(3251, 100), TSLA: basis(2882, 50), NVDA: basis(2882, 80),
      QQQ: { vaultState: 1, ltvBps: 3000, borrowApyBps: 589, supplyApyBps: 650 }, // parked, 18 bps
    });
    const g = rankVaults(v);
    expect(g.slice(0, 4).map((e) => e.ticker)).toEqual(["MSTR", "NVDA", "TSLA", "QQQ"]); // NVDA beats TSLA on TVL at equal APY
    expect(g[0]).toMatchObject({ pos: 1, apyBps: 746, mode: "funding" });
    expect(g[3]).toMatchObject({ pos: 4, apyBps: 18, mode: "parked" });
    expect(g[4].mode).toBe("idle");
    expect(g).toHaveLength(9);
    expect(g.map((e) => e.pos)).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9]);
  });

  it("pole summary names leader, P2, P3 and the gap in points", () => {
    const p = poleSummary(world({ MSTR: basis(3251), TSLA: basis(2882), CRCL: basis(2255) }));
    expect(p?.leader.ticker).toBe("MSTR");
    expect(p?.p2?.ticker).toBe("TSLA");
    expect(p?.p3?.ticker).toBe("CRCL");
    expect(p?.gapPts).toBeCloseTo((746 - 635) / 100, 5);
  });

  it("returns null when every vault is idle so the pole block shows the empty state", () => {
    expect(poleSummary(world({}))).toBeNull();
    const g = rankVaults(world({}));
    expect(g.every((e) => e.apyBps === 0 && e.mode === "idle")).toBe(true);
    expect(g.map((e) => e.ticker)).toEqual([...TICKERS]); // stable ticker order on a full tie
  });
});
