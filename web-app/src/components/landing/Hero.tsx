"use client";

import Link from "next/link";
import { VAULT_META } from "@/constants/vaults";
import { fmt, pct } from "@/lib/format";
import { currentApyBps } from "@/lib/yield";
import { useUiStore } from "@/store/ui-provider";
import { useVaultStore } from "@/store/vault-provider";
import Race from "./Race";

function go(id: string) {
  const reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;
  document.getElementById(id)?.scrollIntoView({ behavior: reduce ? "auto" : "smooth" });
}

export default function Hero() {
  const selected = useUiStore((s) => s.selected);
  const v = useVaultStore((s) => s.vaults[selected]);
  const meta = VAULT_META[selected];
  const apy = currentApyBps(v) / 100;

  const caption =
    v.vaultState === 3
      ? `Hold ${meta.name} and earn about ${fmt(apy, 1)}% a year on top at today's funding, paid in USDC. If ${meta.name} goes up, every bit of that is still yours.`
      : v.mode === "parked"
        ? `Funding on ${meta.name} is below its ${pct(v.hurdleBps)} hurdle right now, so the vault's USDC is supplied on Kamino. It goes back to funding on its own when the 24-hour average clears ${pct(v.hurdleBps + v.enterMarginBps)}.`
        : `Waiting for funding. Funding on ${meta.name} is below its ${pct(v.hurdleBps)} hurdle right now, so the loan is repaid and this vault earns 0.0% for now. It switches on by itself when the 24-hour average clears ${pct(v.hurdleBps + v.enterMarginBps)}.`;

  return (
    <section className="hero" aria-labelledby="hero-t">
      <svg className="rings" viewBox="0 0 1500 1500" aria-hidden="true">
        <g fill="none" stroke="currentColor">
          <circle cx="750" cy="750" r="320" />
          <circle cx="750" cy="750" r="520" />
          <circle cx="750" cy="750" r="720" />
        </g>
      </svg>
      <h1 className="giant" id="hero-t">
        <span className="hero-word">CARRERA</span>
      </h1>
      <p className="lede">
        <span className="serif">Your stocks, with a second engine.</span>
        <span className="s">Deposit the tokenized stocks you already own. Keep every gain, and earn extra in USDC while you hold.</span>
      </p>
      <Race />
      <div className="hcap" aria-live="polite">
        <h2>
          {selected} <span>{meta.name}</span>
        </h2>
        <p>{caption}</p>
        <div className="ctas">
          <Link className="btn" href={`/app?v=${selected}`}>
            Deposit {meta.token}
          </Link>
          <button className="btn o" onClick={() => go("how")}>
            See how it works
          </button>
        </div>
      </div>
    </section>
  );
}
