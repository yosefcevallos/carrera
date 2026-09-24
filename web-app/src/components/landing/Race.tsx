"use client";

import { LANES, TICKERS, VAULT_META, type Ticker } from "@/constants/vaults";
import { realisedYield } from "@/lib/yield";
import { useUiStore } from "@/store/ui-provider";
import { useVaultStore } from "@/store/vault-provider";
import { fmt } from "@/lib/format";

/** Real logos go here as data: URIs once sourced (trademarks, see HANDOFF §10). */
const LOGOS: Partial<Record<Ticker, string>> = {};

function Bubble({ t, copy, maxTvl }: { t: Ticker; copy: number; maxTvl: number }) {
  const v = useVaultStore((s) => s.vaults[t]);
  const selected = useUiStore((s) => s.selected);
  const select = useUiStore((s) => s.select);
  const d = Math.round(56 + Math.sqrt(maxTvl > 0 ? v.tvlUsd / maxTvl : 0) * 72);
  const gap = 70 + ((t.charCodeAt(0) * 37 + t.length * 53 + copy * 41) % 130);
  const y = realisedYield(v, 30);
  const on = selected === t;
  const funding = v.mode === "funding";
  const hidden = copy > 0;
  const logo = LOGOS[t];
  return (
    <span className="slot" style={{ marginRight: gap }}>
      <button
        className={`bub${funding ? "" : " l"}${on ? " on" : ""}`}
        style={{ width: d, height: d }}
        tabIndex={hidden ? -1 : 0}
        aria-hidden={hidden || undefined}
        aria-pressed={hidden ? undefined : on}
        aria-label={hidden ? undefined : `${t}, ${VAULT_META[t].name}, ${fmt(y.apy, 1)}% a year in USDC over the last ${y.days} days`}
        onClick={() => select(t)}
      >
        {logo ? (
          // eslint-disable-next-line @next/next/no-img-element
          <img src={logo} alt="" style={{ width: "56%", height: "56%", objectFit: "contain" }} />
        ) : (
          <span className="bt" style={{ fontSize: Math.round(d * (t.length > 4 ? 0.2 : 0.24)) }}>
            {t}
          </span>
        )}
        <span className="by">{y.apy >= 0 ? "+" : ""}{fmt(y.apy, 1)}%</span>
      </button>
    </span>
  );
}

export default function Race() {
  const vaults = useVaultStore((s) => s.vaults);
  const maxTvl = Math.max(...TICKERS.map((t) => vaults[t].tvlUsd));
  return (
    <>
      <div className="race" role="group" aria-label="Choose a stock">
        {LANES.map((lane, k) => (
          <div className="lane" key={k} style={{ "--dur": `${lane.seconds}s` } as React.CSSProperties}>
            <div className="track">
              {[0, 1].map((half) =>
                [0, 1, 2].map((r) =>
                  lane.tickers.map((t) => <Bubble key={`${half}-${r}-${t}`} t={t} copy={half * 3 + r} maxTvl={maxTvl} />),
                ),
              )}
            </div>
          </div>
        ))}
      </div>
      <p className="race-key">
        <span>
          <b className="f" />
          Earning from funding
        </span>
        <span>
          <b className="l" />
          Parked or idle
        </span>
        <span>Bubble size shows vault size</span>
      </p>
    </>
  );
}
