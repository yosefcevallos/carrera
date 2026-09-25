"use client";

import Link from "next/link";
import { memo, useMemo } from "react";
import { tokenIconSrc } from "@/components/TokenIcon";
import { fmt } from "@/lib/format";
import { rankVaults, type GridEntry } from "@/lib/grid";
import { useVaultStore } from "@/store/vault-provider";

const Item = memo(function Item({ e, hidden }: { e: GridEntry; hidden: boolean }) {
  const y = `${e.apyBps > 0 ? "+" : ""}${fmt(e.apyBps / 100, 1)}%`;
  return (
    <Link
      className="it"
      href={`/app?v=${e.ticker}`}
      tabIndex={hidden ? -1 : undefined}
      aria-hidden={hidden || undefined}
      aria-label={hidden ? undefined : `P${e.pos} ${e.ticker}, ${fmt(e.apyBps / 100, 1)}% a year in USDC, ${e.mode}`}
    >
      <span className="p">P{e.pos}</span>
      <span className="lg">
        {/* eslint-disable-next-line @next/next/no-img-element */}
        <img src={tokenIconSrc(e.ticker)} alt="" loading="lazy" decoding="async" />
      </span>
      <span className="tk">{e.ticker}</span>
      <span className={`y ${e.mode === "funding" ? "f" : "l"}`}>{y}</span>
      <span className="m">{e.mode}</span>
    </Link>
  );
});

/** "Live grid" marquee under the hero: every vault ranked by current APY, duplicated for a seamless 55 s loop. */
export default function LiveGrid() {
  const vaults = useVaultStore((s) => s.vaults);
  const grid = useMemo(() => rankVaults(vaults), [vaults]);
  return (
    <div className="strip">
      <div className="lab">
        <b />
        <span>Live grid</span>
      </div>
      <div className="tape">
        <div className="run">
          {grid.map((e) => (
            <Item key={e.ticker} e={e} hidden={false} />
          ))}
          {grid.map((e) => (
            <Item key={`${e.ticker}-copy`} e={e} hidden />
          ))}
        </div>
      </div>
    </div>
  );
}
