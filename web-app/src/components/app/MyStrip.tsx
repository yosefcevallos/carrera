"use client";

import { TICKERS, VAULT_META } from "@/constants/vaults";
import { fmt, greeting, usd } from "@/lib/format";
import { realisedYield } from "@/lib/yield";
import { usePositionStore } from "@/store/position-provider";
import { useVaultStore } from "@/store/vault-provider";
import { useWalletStore } from "@/store/wallet-provider";

/** Personal strip. Rendered only when connected. All figures derive from the stores. */
export default function MyStrip() {
  const status = useWalletStore((s) => s.status);
  const positions = usePositionStore((s) => s.positions);
  const vaults = useVaultStore((s) => s.vaults);
  if (status !== "connected") return null;

  const held = TICKERS.filter((t) => positions[t].shares > 0);
  const value = held.reduce((a, t) => a + positions[t].stockAmount * vaults[t].priceUsd, 0);
  const earned = held.reduce((a, t) => a + positions[t].usdcEarned, 0);
  const rate = value > 0 ? held.reduce((a, t) => a + positions[t].stockAmount * vaults[t].priceUsd * realisedYield(vaults[t], 30).apy, 0) / value : 0;

  return (
    <div className="mystrip">
      <span className="serif">{greeting()}</span>
      <span>
        <small>Your deposits</small>
        <b className="num">{usd(value)}</b>
        <em>{held.length ? held.map((t) => `${fmt(positions[t].stockAmount)} ${VAULT_META[t].token}`).join(", ") : "Nothing yet"}</em>
      </span>
      <span>
        <small>Earned so far</small>
        <b className={`num${earned >= 0 ? " red" : ""}`}>{earned >= 0 ? "+" : ""}{usd(earned, 2)}</b>
        <em>USDC, paid when you withdraw</em>
      </span>
      <span>
        <small>Your realised rate, 30d</small>
        <b className="num">{fmt(rate, 1)}%</b>
        <em>a year</em>
      </span>
    </div>
  );
}
