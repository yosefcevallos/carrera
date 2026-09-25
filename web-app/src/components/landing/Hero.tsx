"use client";

import Link from "next/link";
import { useEffect, useMemo, useRef, useState } from "react";
import { VAULT_META } from "@/constants/vaults";
import TokenIcon from "@/components/TokenIcon";
import { fmt } from "@/lib/format";
import { poleSummary } from "@/lib/grid";
import { useVaultStore } from "@/store/vault-provider";

const STREAKS = [
  { t: "58%", h: "3px", o: 0.35, b: "3px", s: "1.1s" },
  { t: "66%", h: "2px", o: 0.5, b: "1.5px", s: "0.8s" },
  { t: "72%", h: "6px", o: 0.18, b: "6px", s: "1.4s" },
  { t: "80%", h: "2px", o: 0.4, b: "2px", s: "0.7s" },
  { t: "88%", h: "10px", o: 0.12, b: "9px", s: "1.8s" },
  { t: "47%", h: "1px", o: 0.3, b: "1px", s: "1.6s" },
];

const Brackets = () => (
  <>
    <i />
    <i />
    <i />
    <i />
  </>
);

/**
 * Dark cinematic hero per docs/frontend-handoff/hero-v2.html. `hasFootage` is decided on the
 * server from public/hero.mp4; without it the layered stand-in renders.
 */
export default function Hero({
  hasFootage = false,
  showSlotTag = false,
}: {
  hasFootage?: boolean;
  showSlotTag?: boolean;
}) {
  const vaults = useVaultStore((s) => s.vaults); // stable slice: the store replaces it only when a poll lands
  const pole = useMemo(() => poleSummary(vaults), [vaults]);
  const ref = useRef<HTMLElement>(null);
  const [off, setOff] = useState(false);
  useEffect(() => {
    const el = ref.current;
    if (!el || typeof IntersectionObserver === "undefined") return;
    const io = new IntersectionObserver(([e]) => setOff(!e.isIntersecting), {
      threshold: 0.05,
    });
    io.observe(el);
    return () => io.disconnect();
  }, []);

  return (
    <section
      className={`hero2${off ? " off" : ""}`}
      aria-labelledby="hero-t"
      ref={ref}
    >
      {hasFootage ? (
        <video
          className="footage"
          autoPlay
          muted
          loop
          playsInline
          poster="/hero-poster.jpg"
          aria-hidden="true"
        >
          <source src="/hero.mp4" type="video/mp4" />
        </video>
      ) : (
        <div className="standin" aria-hidden="true">
          <div className="sky" />
          <div
            className="crowd"
            style={{ "--s": "2.2s" } as React.CSSProperties}
          />
          {STREAKS.map((x, i) => (
            <div
              key={i}
              className="streak"
              style={
                {
                  "--t": x.t,
                  "--h": x.h,
                  "--o": x.o,
                  "--b": x.b,
                  "--s": x.s,
                } as React.CSSProperties
              }
            />
          ))}
          <div className="tail" />
          <div className="car" />
        </div>
      )}
      <div className="shade" />
      <div className="grain" aria-hidden="true" />
      {showSlotTag && (
        <span className="slot-tag" aria-hidden="true">
          <i />
          Footage slot: B&amp;W vintage grand prix loop
        </span>
      )}

      <div className="word" aria-hidden="true">
        <span>CARRERA</span>
      </div>

      <div className="base">
        <div className="copy">
          <span className="live">
            <b />
            Live on Solana
          </span>
          <h1 id="hero-t">Earn yield on your stocks.</h1>
          <p className="sub">
            Deposit the tokenized stocks you already own. Keep every gain, and
            earn extra yield in USDC.
          </p>
          <div className="ctas">
            <Link className="btn" href="/app">
              Launch app
              <svg className="arr" viewBox="0 0 16 16" aria-hidden="true">
                <path d="M3 8h9M8 3.5 12.5 8 8 12.5" fill="none" stroke="currentColor" strokeWidth="2" />
              </svg>
            </Link>
            <Link className="link" href="/docs">
              How it works
            </Link>
          </div>
        </div>

        <div className="pole brk">
          <Brackets />
          <div className="h">
            <span>Pole position</span>
            <span>Updated hourly</span>
          </div>
          {pole ? (
            <>
              <div className="row">
                <TokenIcon t={pole.leader.ticker} size={34} />
                <span>
                  <span className="tk">{pole.leader.ticker}</span>
                  <br />
                  <span className="co">
                    {VAULT_META[pole.leader.ticker].name}
                  </span>
                </span>
                <span className="y">
                  <b className="num">{fmt(pole.leader.apyBps / 100, 1)}%</b>
                  <span>a year, in USDC</span>
                </span>
              </div>
              <div className="gap">
                <span>
                  {pole.p2
                    ? `P2 ${pole.p2.ticker} ${fmt(pole.p2.apyBps / 100, 1)}%`
                    : ""}
                </span>
                <span>
                  {pole.p3
                    ? `P3 ${pole.p3.ticker} ${fmt(pole.p3.apyBps / 100, 1)}%`
                    : ""}
                </span>
                <span>Gap +{fmt(pole.gapPts, 1)}</span>
              </div>
            </>
          ) : (
            <div className="row">
              <span className="co">No vault is earning right now</span>
            </div>
          )}
        </div>
      </div>
    </section>
  );
}
