import { Suspense } from "react";
import { TICKERS } from "@/constants/vaults";
import { tokenIconSrc } from "@/components/TokenIcon";
import type { Metadata } from "next";
import AppBar from "@/components/app/AppBar";
import AppMain from "@/components/app/AppMain";

export const metadata: Metadata = { title: "Carrera app — Vaults" };

/** Fintech v1 (docs/frontend-handoff/app-fintech-v1.html): the `.app2` scope carries its own dark palette. */
export default function AppPage() {
  return (
    <div className="app2 dots">
      {TICKERS.map((t) => (
        <link key={t} rel="preload" as="image" href={tokenIconSrc(t)} />
      ))}
      <AppBar />
      <Suspense>
        <AppMain />
      </Suspense>
    </div>
  );
}
