"use client";

import { TICKERS } from "@/constants/vaults";
import { isLive, signedPct, stateLabel } from "@/lib/ops-math";
import { useOpsStore } from "@/store/ops-provider";

export default function BookList() {
  const books = useOpsStore((s) => s.books);
  const selected = useOpsStore((s) => s.selected);
  const select = useOpsStore((s) => s.select);
  const live = TICKERS.filter((t) => isLive(books[t].state)).length;

  return (
    <nav className="ops-books" aria-label="Books">
      <h3>
        <span>Books</span>
        <span>{live} live</span>
      </h3>
      {TICKERS.map((t) => {
        const b = books[t];
        const net = b.carry.ann_net_bps;
        return (
          <button key={t} aria-pressed={t === selected} onClick={() => select(t)}>
            <span>
              {t}x basis
              <small>
                {stateLabel[b.state]}
                {b.state === "winding" || b.state === "unwinding" ? `, step ${b.step}` : ""}
              </small>
            </span>
            <span className={`net num${net > 0 ? " pos" : ""}`}>{b.state === "idle" ? "—" : signedPct(net)}</span>
          </button>
        );
      })}
    </nav>
  );
}
