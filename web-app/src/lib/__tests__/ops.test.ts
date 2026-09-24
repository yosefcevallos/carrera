import { describe, expect, it } from "vitest";
import { Keypair } from "@solana/web3.js";
import { TICKERS } from "@/constants/vaults";
import { fetchOps, fetchOpsHistory } from "@/lib/fetch-ops";
import { discriminator, pauseIx, rebalanceToKaminoIx, rebalanceToPhoenixIx, UNWIND_EMERGENCY, unwindStartIx, vaultKeys } from "@/lib/chain/ix";
import { mockOpsAction, nyseOpen, resetOpsWorld } from "@/lib/mock/ops";
import { ageLabel, basisBps, breaksFirst, downsample, signedPct, signedQty, signedUsd, spotNotionalUsd } from "@/lib/ops-math";
import type { Leg } from "@/lib/ops-types";
import { createOpsStore } from "@/store/ops-store";

describe("fetchOps (mock)", () => {
  it("returns a book for every ticker and a keeper block", async () => {
    resetOpsWorld();
    const snap = await fetchOps();
    for (const t of TICKERS) {
      const b = snap.books[t];
      expect(b.symbol).toBe(t);
      expect(["idle", "parked", "winding", "basis", "unwinding"]).toContain(b.state);
      expect(Array.isArray(b.legs)).toBe(true);
      expect(typeof b.rule.hurdle_bps).toBe("number");
      expect(b.rule.enter_bps).toBe(b.rule.hurdle_bps + 200);
      expect(b.rule.exit_bps).toBe(b.rule.hurdle_bps - 100);
    }
    expect(snap.keeper.instance_id).not.toBe("");
    expect(snap.keeper.alerts.length).toBeGreaterThan(0);
    // Every state the dashboard renders is present, including a stuck Winding vault.
    const states = new Set(TICKERS.map((t) => snap.books[t].state));
    expect(states).toContain("basis");
    expect(states).toContain("parked");
    expect(states).toContain("idle");
    expect(states).toContain("winding");
    expect(snap.keeper.alerts.some((a) => a.vault === "NVDA" && a.level === "crit")).toBe(true);
  });

  it("basis books carry three legs with the short marked as breaking first", async () => {
    resetOpsWorld();
    const { books } = await fetchOps();
    const kinds = books.TSLA.legs.map((l) => l.kind);
    expect(kinds).toEqual(["long_spot", "borrow_usdc", "short_perp"]);
    expect(books.TSLA.net_delta.qty).toBe(0);
    expect(breaksFirst(books.TSLA.legs).leg?.kind).toBe("short_perp");
    expect(books.MSTR.legs.map((l) => l.kind)).toEqual(["borrow_usdc", "supply_usdc"]);
    expect(books.SPY.legs).toEqual([]);
    expect(books.SPY.margin_bps).toBeNull();
  });

  it("hydrates the skeleton store without changing its shape", async () => {
    resetOpsWorld();
    const store = createOpsStore();
    const before = Object.keys(store.getState().books).sort();
    const snap = await fetchOps();
    store.getState().setBooks(snap.books);
    store.getState().setKeeper(snap.keeper);
    expect(Object.keys(store.getState().books).sort()).toEqual(before);
    expect(store.getState().books.TSLA.state).toBe("basis");
    const hist = await fetchOpsHistory("TSLA", 168);
    expect(hist.length).toBeGreaterThan(1000);
    expect(hist[hist.length - 1].state).toBe("basis");
    store.getState().setHistory("TSLA", hist);
    expect(store.getState().history.TSLA.length).toBe(hist.length);
    store.getState().resetOps();
    expect(store.getState().books.TSLA.legs).toEqual([]);
    expect(store.getState().history.TSLA).toEqual([]);
    expect(store.getState().keeper.instance_id).toBe("");
  });

  it("mock actions mutate the book and refuse when the state does not allow them", async () => {
    resetOpsWorld();
    const before = (await fetchOps()).books.TSLA.ltv_bps;
    await mockOpsAction("rebalance_to_kamino", "TSLA");
    expect((await fetchOps()).books.TSLA.ltv_bps).toBe(before - 300);
    await expect(mockOpsAction("rebalance_to_phoenix", "SPY")).rejects.toThrow(/not in the basis trade/);
    await mockOpsAction("unwind_emergency", "GOOGL");
    expect((await fetchOps()).books.GOOGL.state).toBe("unwinding");
  });

  it("nyseOpen follows the New York session", () => {
    expect(nyseOpen(new Date("2026-09-23T14:00:00Z"))).toBe(true); // Wed 10:00 EDT
    expect(nyseOpen(new Date("2026-09-23T21:00:00Z"))).toBe(false); // Wed 17:00 EDT
    expect(nyseOpen(new Date("2026-09-26T15:00:00Z"))).toBe(false); // Saturday
  });
});

describe("ops math", () => {
  const legs: Leg[] = [
    { kind: "long_spot", venue: "Kamino", size: 1_000 * 1e8, mark_e6: 412_300_000, rate_bps: 0, liq_price_e6: 245_100_000, liq_distance_bps: 4100 },
    { kind: "borrow_usdc", venue: "Kamino", size: 200_000 * 1e6, mark_e6: 1e6, rate_bps: -820, liq_price_e6: 245_100_000, liq_distance_bps: 4100 },
    { kind: "short_perp", venue: "Phoenix", size: 1_000 * 1e8, mark_e6: 413_950_000, rate_bps: 1260, liq_price_e6: 531_400_000, liq_distance_bps: 2800 },
  ];
  it("basis, notional and the leg that breaks first", () => {
    expect(basisBps(legs)).toBe(40);
    expect(spotNotionalUsd(legs)).toBeCloseTo(412_300, 0);
    expect(breaksFirst(legs)).toMatchObject({ distanceBps: 2800 });
    expect(breaksFirst([]).leg).toBeUndefined();
    expect(basisBps([legs[0]])).toBe(0);
  });
  it("formats signed quantities, dollars and percentages", () => {
    expect(signedQty(0.4)).toBe("+0.40");
    expect(signedQty(-1.25)).toBe("−1.25");
    expect(signedQty(0)).toBe("0.00");
    expect(signedUsd(2140)).toBe("+$2,140");
    expect(signedUsd(-3.5)).toBe("−$3.50");
    expect(signedPct(860)).toBe("+8.6%");
    expect(signedPct(-820)).toBe("−8.2%");
    expect(signedPct(40, 2)).toBe("+0.40%");
  });
  it("labels age and downsamples long series", () => {
    const now = 1_000_000;
    expect(ageLabel(0, now)).toBe("—");
    expect(ageLabel(now - 22 * 60, now)).toBe("22m ago");
    expect(ageLabel(now - 3 * 3600, now)).toBe("3h ago");
    expect(ageLabel(now - 19 * 86_400, now)).toBe("19d ago");
    const series = Array.from({ length: 10_080 }, (_, i) => i);
    const ds = downsample(series);
    expect(ds.length).toBeLessThanOrEqual(602);
    expect(ds[0]).toBe(0);
    expect(ds[ds.length - 1]).toBe(10_079);
    expect(downsample([1, 2, 3])).toEqual([1, 2, 3]);
  });
});

describe("ops instruction builders", () => {
  const signer = Keypair.generate().publicKey;
  const k = vaultKeys(Keypair.generate().publicKey);

  it("rebalance cranks carry only the discriminator and three accounts", () => {
    const a = rebalanceToKaminoIx(signer, k);
    const b = rebalanceToPhoenixIx(signer, k);
    expect(a.data).toEqual(Buffer.from(discriminator("rebalance_to_kamino")));
    expect(b.data).toEqual(Buffer.from(discriminator("rebalance_to_phoenix")));
    expect(a.data).not.toEqual(b.data);
    expect(a.keys.map((m) => [m.isSigner, m.isWritable])).toEqual([[true, false], [false, false], [false, true]]);
    expect(a.keys[1].pubkey.equals(k.registry)).toBe(true);
    expect(a.keys[2].pubkey.equals(k.vault)).toBe(true);
  });

  it("unwind_start encodes the reason as one u8 after the discriminator", () => {
    const ix = unwindStartIx(signer, k, UNWIND_EMERGENCY);
    expect(ix.data).toHaveLength(9);
    expect(ix.data.subarray(0, 8)).toEqual(Buffer.from(discriminator("unwind_start")));
    expect(ix.data[8]).toBe(2);
  });

  it("pause writes the registry with the signer first", () => {
    const ix = pauseIx(signer, k.registry);
    expect(ix.data).toEqual(Buffer.from(discriminator("pause")));
    expect(ix.keys).toHaveLength(2);
    expect(ix.keys[1].isWritable).toBe(true);
  });
});
