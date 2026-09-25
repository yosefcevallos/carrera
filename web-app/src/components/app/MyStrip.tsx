"use client";

import { TICKERS, VAULT_META } from "@/constants/vaults";
import { fmt, greeting, usd } from "@/lib/format";
import TokenIcon from "@/components/TokenIcon";
import { currentApyBps } from "@/lib/yield";
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
  const rate = value > 0 ? held.reduce((a, t) => a + positions[t].stockAmount * vaults[t].priceUsd * (currentApyBps(vaults[t]) / 100), 0) / value : 0;

  return (
    <div className="mystrip">
      <span className="serif">{greeting()}</span>
      <span>
        <small>Your deposits</small>
        <b className="num">{usd(value)}</b>
        <em className="pos-list">
          {held.length
            ? held.map((t) => (
                <span key={t}>
                  <TokenIcon t={t} size={16} /> {fmt(positions[t].stockAmount, 4)} {VAULT_META[t].token}
                </span>
              ))
            : "Nothing yet"}
        </em>
      </span>
      <span>
        <small>Earned so far</small>
        <b className={`num${earned >= 0 ? " red" : ""}`}>{earned >= 0 ? "+" : ""}{usd(earned, 2)}</b>
        <em>USDC, paid when you withdraw</em>
      </span>
      <span>
        <small>Your earning rate</small>
        <b className="num">{fmt(rate, 1)}%</b>
        <em>a year, in USDC</em>
      </span>
    </div>
  );
}
