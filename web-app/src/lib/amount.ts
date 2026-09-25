// Exact conversions between typed decimal strings and raw base units. No floats: xStocks carry
// the Token-2022 ScaledUiAmount extension, so a wallet's `uiAmount` can differ from raw/10^decimals
// and rounding through a double can produce a raw amount the wallet does not hold.

/** "0.0130312" with 8 decimals → 1303120n. Extra fractional digits are truncated. Empty → 0n. */
export function parseToRaw(text: string, decimals: number): bigint {
  const s = text.trim().replace(/,/g, "");
  if (!s || !/^\d*\.?\d*$/.test(s)) return 0n;
  const [whole = "", frac = ""] = s.split(".");
  const fracPadded = (frac + "0".repeat(decimals)).slice(0, decimals);
  return BigInt(whole || "0") * 10n ** BigInt(decimals) + BigInt(fracPadded || "0");
}

/** 1295716n with 8 decimals → "0.01295716". Trailing zeros trimmed; integers have no point. */
export function formatRaw(raw: bigint, decimals: number): string {
  const neg = raw < 0n;
  const abs = neg ? -raw : raw;
  const base = 10n ** BigInt(decimals);
  const whole = abs / base;
  const frac = (abs % base).toString().padStart(decimals, "0").replace(/0+$/, "");
  return (neg ? "-" : "") + whole.toString() + (frac ? "." + frac : "");
}

/** Display number for a raw amount: raw / 10^decimals. Only for display and estimates. */
export function rawToNumber(raw: bigint, decimals: number): number {
  return Number(raw) / 10 ** decimals;
}
