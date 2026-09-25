import { describe, expect, it } from "vitest";
import { TICKERS } from "@/constants/vaults";
import { fetchPositions } from "@/lib/fetch-positions";
import { fetchVaults } from "@/lib/fetch-vaults";
import { mockDeposit, mockRedeem, mockRequestExit } from "@/lib/mock";
import { DEMO_WALLET, getWorld, resetWorld } from "@/lib/mock/world";
import { createVaultStore } from "@/store/vault-store";
import { createPositionStore } from "@/store/position-store";

describe("fetchVaults (mock)", () => {
  it("returns a record for every ticker with every field present", async () => {
    resetWorld();
    const snap = await fetchVaults();
    for (const t of TICKERS) {
      const v = snap.vaults[t];
      expect(v).toBeDefined();
      expect(typeof v.priceUsd).toBe("number");
      expect(typeof v.hurdleBps).toBe("number");
      expect(Array.isArray(v.sharePriceHistory)).toBe(true);
      expect(v.sharePriceHistory.length).toBe(v.ageDays + 1);
      expect(v.fundingSamples.length).toBe(168);
      for (const s of v.fundingSamples) {
        expect(typeof s.ts).toBe("number");
        expect(typeof s.rateScaled).toBe("number");
      }
      expect(v.fundingSamples[167].ts).toBeGreaterThan(v.fundingSamples[0].ts);
      expect(["funding", "parked", "idle"]).toContain(v.mode);
    }
    expect(snap.protocol.tvlUsd).toBeGreaterThan(0);
    expect(snap.protocol.vaultsInFunding).toBe(TICKERS.filter((t) => snap.vaults[t].mode === "funding").length);
  });

  it("is deterministic across calls for static fields", async () => {
    resetWorld();
    const a = await fetchVaults();
    resetWorld();
    const b = await fetchVaults();
    for (const t of TICKERS) expect(a.vaults[t].hurdleBps).toBe(b.vaults[t].hurdleBps);
  });

  it("hydrates the skeleton store without changing its shape", async () => {
    resetWorld();
    const store = createVaultStore();
    const before = Object.keys(store.getState().vaults).sort();
    const snap = await fetchVaults();
    store.getState().setVaults(snap.vaults);
    store.getState().setProtocol(snap.protocol);
    expect(Object.keys(store.getState().vaults).sort()).toEqual(before);
    expect(store.getState().vaults.TSLA.priceUsd).toBeGreaterThan(0);
    store.getState().resetVaults();
    expect(store.getState().vaults.TSLA.priceUsd).toBe(0);
  });
});

describe("fetchPositions (mock)", () => {
  it("returns every ticker in balances, positions and pendingExits", async () => {
    resetWorld();
    const snap = await fetchPositions(DEMO_WALLET);
    for (const t of TICKERS) {
      expect(typeof snap.balances[t]).toBe("number");
      expect(snap.balancesRaw[t]).toMatch(/^\d+$/);
      expect(snap.sharesRaw[t]).toMatch(/^\d+$/);
      expect(snap.decimals[t]).toBe(8);
      expect(snap.positions[t]).toMatchObject({ shares: expect.any(Number), stockAmount: expect.any(Number), usdcEarned: expect.any(Number) });
      expect(snap.pendingExits[t]).toMatchObject({ shares: expect.any(Number), ready: expect.any(Boolean), readyAt: expect.any(Number) });
    }
  });

  it("deposit → exit → redeem round trip moves stock through the position store shape", async () => {
    resetWorld();
    const store = createPositionStore();
    const w = getWorld();
    const start = w.balances.NVDA;
    await mockDeposit("NVDA", 2);
    let snap = await fetchPositions(DEMO_WALLET);
    store.getState().setBalances(snap.balances);
    store.getState().setPositions(snap.positions);
    expect(store.getState().balances.NVDA).toBeCloseTo(start - 2);
    expect(store.getState().positions.NVDA.stockAmount).toBeCloseTo(2);
    await mockRequestExit("NVDA", 2);
    w.pendingExits.NVDA.readyAt = 0;
    snap = await fetchPositions(DEMO_WALLET);
    store.getState().setPendingExits(snap.pendingExits);
    expect(store.getState().pendingExits.NVDA.ready).toBe(true);
    const out = await mockRedeem("NVDA");
    expect(out.stock).toBeCloseTo(2);
    expect(getWorld().balances.NVDA).toBeCloseTo(start);
  });
});
