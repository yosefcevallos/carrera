"use client";

import { TICKERS } from "@/constants/vaults";
import { fmt, pct, usd, usdCompact } from "@/lib/format";
import { useVaultStore } from "@/store/vault-provider";

export default function ProtocolStats() {
  const p = useVaultStore((s) => s.protocol);
  return (
    <dl className="pstats">
      <div>
        <dt>Total value locked</dt>
        <dd className="num">{usdCompact(p.tvlUsd)}</dd>
      </div>
      <div>
        <dt>Realised yield, 30d</dt>
        <dd className="red num">
          {pct(p.avgYieldBps)}
          <small>a year in USDC, TVL-weighted</small>
        </dd>
      </div>
      <div>
        <dt>Earning from funding</dt>
        <dd className="num">
          {p.vaultsInFunding}
          <small>of {TICKERS.length} vaults, the rest are parked or idle</small>
        </dd>
      </div>
      <div>
        <dt>USDC paid out, 24h</dt>
        <dd className="num">{usd(p.usdcPaid24h)}</dd>
      </div>
      <div>
        <dt>Depositors</dt>
        <dd className="num">{fmt(p.depositors, 0)}</dd>
      </div>
    </dl>
  );
}
