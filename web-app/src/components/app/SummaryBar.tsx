"use client";

import { TICKERS } from "@/constants/vaults";
import { weightedApyBps } from "@/lib/app2";
import { fmt, usd } from "@/lib/format";
import { usePositionStore } from "@/store/position-provider";
import { useVaultStore } from "@/store/vault-provider";
import { useWalletStore } from "@/store/wallet-provider";
import Corners from "./Corners";

/**
 * One bracketed bar: balance, earned, your APY, protocol TVL. Without a wallet the personal cells
 * give way to the protocol figures and a connect button.
 */
export default function SummaryBar({ onConnect }: { onConnect: () => void }) {
  const status = useWalletStore((s) => s.status);
  const protocol = useVaultStore((s) => s.protocol);
  const vaults = useVaultStore((s) => s.vaults);
  const positions = usePositionStore((s) => s.positions);
  const connected = status === "connected";

  const held = TICKERS.filter((t) => positions[t].shares > 0);
  const value = held.reduce((a, t) => a + positions[t].stockAmount * vaults[t].priceUsd, 0);
  const earned = held.reduce((a, t) => a + positions[t].usdcEarned, 0);
  const apy = weightedApyBps(vaults, positions);
  const funding = TICKERS.filter((t) => vaults[t].mode === "funding").length;

  return (
    <div className="sum brk">
      <Corners />
      {connected ? (
        <>
          <div>
            <div className="k">Your balance</div>
            <div className="v mono">{usd(value, 2)}</div>
            <div className="s">
              {held.length} position{held.length === 1 ? "" : "s"}
            </div>
          </div>
          <div>
            <div className="k">Earned</div>
            <div className={`v mono${earned > 0 ? " pos" : " muted"}`}>
              {fmt(earned, 2)}
              <small>USDC</small>
            </div>
            <div className="s">Paid on withdrawal</div>
          </div>
          <div>
            <div className="k">Your APY</div>
            <div className="v mono">{fmt(apy / 100, 2)}%</div>
            <div className="s">Weighted by position</div>
          </div>
        </>
      ) : (
        <>
          <div>
            <div className="k">Vaults funding</div>
            <div className="v mono">
              {funding}
              <small>/ {TICKERS.length}</small>
            </div>
            <div className="s">Earning from Phoenix funding</div>
          </div>
          <div className="cta2">
            <div className="k">Your balance</div>
            <button className="go ghost" onClick={onConnect}>
              Connect wallet
            </button>
            <div className="s">See your positions and earnings</div>
          </div>
          <div>
            <div className="k">Your APY</div>
            <div className="v mono muted">—</div>
            <div className="s">Weighted by position</div>
          </div>
        </>
      )}
      <div>
        <div className="k">Protocol TVL</div>
        <div className="v mono">{usd(protocol.tvlUsd, 2)}</div>
        <div className="s">
          {funding} / {TICKERS.length} vaults funding
        </div>
      </div>
    </div>
  );
}
