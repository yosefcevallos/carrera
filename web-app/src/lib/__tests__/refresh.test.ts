import { describe, expect, it, vi } from "vitest";
import { TICKERS } from "@/constants/vaults";
import { refreshAfterAction, type RefreshSetters } from "@/lib/use-refresh";
import { mockFetchPositions, mockFetchVaults } from "@/lib/mock";
import { filled } from "@/lib/zeroed";

const setters = (): RefreshSetters & Record<string, ReturnType<typeof vi.fn>> => ({
  setProtocol: vi.fn(), setVaults: vi.fn(), setBalances: vi.fn(), setPositions: vi.fn(), setExits: vi.fn(), setRaw: vi.fn(),
});

describe("refreshAfterAction", () => {
  it("fetches vaults without history and positions without the indexer, then commits through every setter", async () => {
    const s = setters();
    const deps = { fetchVaults: vi.fn(mockFetchVaults), fetchPositions: vi.fn((a: string) => mockFetchPositions()) };
    const known = filled(TICKERS, () => []);
    await refreshAfterAction("wallet1", s, known, deps);
    expect(deps.fetchVaults).toHaveBeenCalledWith({ history: false });
    expect(deps.fetchPositions).toHaveBeenCalledWith("wallet1", { indexer: false, knownExits: known });
    for (const k of ["setProtocol", "setVaults", "setBalances", "setPositions", "setExits", "setRaw"]) expect(s[k]).toHaveBeenCalledTimes(1);
  });

  it("never throws: a failed fetcher is logged and the other half still commits", async () => {
    const s = setters();
    const err = vi.spyOn(console, "error").mockImplementation(() => {});
    const deps = { fetchVaults: vi.fn(async () => { throw new Error("rpc down"); }), fetchPositions: vi.fn(() => mockFetchPositions()) };
    await expect(refreshAfterAction("wallet1", s, undefined, deps)).resolves.toBeUndefined();
    expect(s.setVaults).not.toHaveBeenCalled();
    expect(s.setBalances).toHaveBeenCalledTimes(1);
    expect(err).toHaveBeenCalled();
    err.mockRestore();
  });

  it("skips positions when no wallet is connected", async () => {
    const s = setters();
    const deps = { fetchVaults: vi.fn(mockFetchVaults), fetchPositions: vi.fn(() => mockFetchPositions()) };
    await refreshAfterAction("", s, undefined, deps);
    expect(deps.fetchPositions).not.toHaveBeenCalled();
    expect(s.setVaults).toHaveBeenCalledTimes(1);
    expect(s.setBalances).not.toHaveBeenCalled();
  });
});
