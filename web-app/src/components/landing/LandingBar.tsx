"use client";

import Link from "next/link";
import Mark from "@/components/Mark";

export default function LandingBar() {
  return (
    <header className="bar">
      <div className="l" aria-hidden="true" />
      <Mark />
      <div className="r">
        <Link className="chip" href="/app">
          Launch app
        </Link>
      </div>
    </header>
  );
}
