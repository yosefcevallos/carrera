"use client";

import Link from "next/link";
import Mark from "@/components/Mark";

export default function LandingBar() {
  return (
    <header className="bar">
      <nav className="l" aria-label="Sections">
        <Link href="/docs">How it works</Link>
        <Link href="/docs">Docs</Link>
      </nav>
      <Mark />
      <div className="r">
        <Link className="chip" href="/app">
          Launch app
        </Link>
      </div>
    </header>
  );
}
