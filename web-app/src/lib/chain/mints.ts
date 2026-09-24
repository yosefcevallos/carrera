import { PublicKey } from "@solana/web3.js";
import { TICKERS, type Ticker } from "@/constants/vaults";

/**
 * xStock mint per ticker. Set NEXT_PUBLIC_XSTOCK_MINTS as JSON {"TSLA":"<mint>",...}.
 * Unset tickers fall back to a deterministic placeholder so PDAs still derive in dev.
 */
function load(): Record<Ticker, PublicKey> {
  let cfg: Partial<Record<Ticker, string>> = {};
  try {
    cfg = JSON.parse(process.env.NEXT_PUBLIC_XSTOCK_MINTS ?? "{}");
  } catch {
    cfg = {};
  }
  const out = {} as Record<Ticker, PublicKey>;
  for (const t of TICKERS) {
    const s = cfg[t];
    out[t] = s ? new PublicKey(s) : PublicKey.findProgramAddressSync([Buffer.from("placeholder"), Buffer.from(t)], PublicKey.default)[0];
  }
  return out;
}

export const XSTOCK_MINTS = load();
