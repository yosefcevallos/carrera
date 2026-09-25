import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { fundingRows, toScaled, type PhoenixRatesResponse } from "../src/sources/phoenix.js";

const fixture = JSON.parse(
  readFileSync(new URL("../../keeper/tests/fixtures/phoenix_funding_tsla.json", import.meta.url), "utf8"),
) as PhoenixRatesResponse;

describe("phoenix funding conversion", () => {
  it("scales percent-per-hour to bps × 1e6 (program FUNDING_SCALE)", () => {
    expect(toScaled("0.003967")).toBe(396_700); // ≈ 34.7% annualised
    expect(toScaled("-0.000261")).toBe(-26_100);
    expect(toScaled("0")).toBe(0);
  });

  it("maps the captured TSLA fixture to rows keyed on vault and ts", () => {
    const rows = fundingRows("TSLA", fixture);
    expect(rows.length).toBe(fixture.rates.length);
    expect(rows[0]).toEqual({
      vault_symbol: "TSLA",
      ts: new Date(fixture.rates[0].timestamp * 1000).toISOString(),
      rate_hourly_scaled: toScaled(fixture.rates[0].fundingRatePercentage),
    });
    // timestamps are hourly and unique
    const ts = new Set(rows.map((r) => r.ts));
    expect(ts.size).toBe(rows.length);
  });
});
