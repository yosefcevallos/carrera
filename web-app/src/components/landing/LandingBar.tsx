"use client";

import Link from "next/link";
import Mark from "@/components/Mark";

function go(id: string) {
  const reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;
  document.getElementById(id)?.scrollIntoView({ behavior: reduce ? "auto" : "smooth" });
}

export default function LandingBar() {
  return (
    <header className="bar">
      <nav className="l" aria-label="Sections">
        <button onClick={() => go("how")}>How it works</button>
        <button onClick={() => go("earn")}>Earnings</button>
        <button onClick={() => go("safety")}>Safety</button>
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
