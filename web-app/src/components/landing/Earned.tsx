"use client";

import { TICKERS, VAULT_META } from "@/constants/vaults";
import { fmt } from "@/lib/format";
import { modeLong, trailingYield } from "@/lib/yield";
import { useUiStore } from "@/store/ui-provider";
import { useVaultStore } from "@/store/vault-provider";

/** Replaces the projected-earnings calculator: only trailing realised growth is shown (DECISIONS D5). */
export default function Earned() {
  const selected = useUiStore((s) => s.selected);
  const select = useUiStore((s) => s.select);
  const v = useVaultStore((s) => s.vaults[selected]);
  const meta = VAULT_META[selected];
  const y7 = trailingYield(v.sharePriceHistory, 7, v.priceUsd);
  const y30 = trailingYield(v.sharePriceHistory, 30, v.priceUsd);
  const yAll = trailingYield(v.sharePriceHistory, 3650, v.priceUsd);
  const young = v.ageDays < 7;

  return (
    <section className="calc" id="earn" aria-labelledby="calc-t">
      <div className="wrap">
        <div>
          <h2 className="h2" id="calc-t">
            What this vault has earned
          </h2>
          <p className="intro">Pick a stock. These are realised numbers from the vault&apos;s share price, not a forecast. Rates move every hour.</p>
        </div>
        <div className="calcbox">
          <span className="lab" id="chips-l">
            Stock
          </span>
          <div className="chips" role="group" aria-labelledby="chips-l">
            {TICKERS.map((t) => (
              <button key={t} aria-pressed={t === selected} onClick={() => select(t)}>
                {t}
              </button>
            ))}
          </div>
          <div className="amt num">
            1 {meta.token} {v.usdcPerShare >= 0 ? "+" : "−"} {fmt(Math.abs(v.usdcPerShare), 3)} USDC
            <small>
              What one share is worth today. {modeLong[v.mode]}.
            </small>
          </div>
          <dl className="result">
            {young ? (
              <div>
                <dt>Since inception</dt>
                <dd className="red num">{fmt(yAll.apy, 1)}%</dd>
                <small>
                  a year, in USDC, over {yAll.days} {yAll.days === 1 ? "day" : "days"}
                </small>
              </div>
            ) : (
              <>
                <div>
                  <dt>Last 7 days</dt>
                  <dd className="red num">{fmt(y7.apy, 1)}%</dd>
                  <small>a year, in USDC</small>
                </div>
                <div>
                  <dt>Last 30 days</dt>
                  <dd className="red num">{y30.sinceInception ? "—" : `${fmt(y30.apy, 1)}%`}</dd>
                  <small>{y30.sinceInception ? `vault is ${v.ageDays} days old` : "a year, in USDC"}</small>
                </div>
                <div>
                  <dt>Since inception</dt>
                  <dd className="num">{fmt(yAll.apy, 1)}%</dd>
                  <small>{yAll.days} days</small>
                </div>
              </>
            )}
          </dl>
          <p className="fine">
            You keep 100% of {meta.name}&apos;s price moves. Yields are after Carrera&apos;s 15% fee on earnings. Not a promise: rates move every hour, and when funding is low a vault
            parks its USDC on Kamino or repays the loan and earns nothing until funding recovers.
          </p>
        </div>
      </div>
    </section>
  );
}
