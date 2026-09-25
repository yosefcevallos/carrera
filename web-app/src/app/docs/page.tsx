import type { Metadata } from "next";
import Link from "next/link";
import LandingBar from "@/components/landing/LandingBar";

export const metadata: Metadata = { title: "Carrera — Docs" };

export default function DocsPage() {
  return (
    <>
      <LandingBar />
      <main className="wrap docs">
        <h1 className="h2">Docs coming soon</h1>
        <p className="intro">
          The mechanism, risks, fees and parameters are being written up. Until then, the technical specification and the program interface
          contract live in the repository.
        </p>
        <p className="intro">
          <Link className="link-ink" href="/">
            Back to Carrera
          </Link>
        </p>
      </main>
    </>
  );
}
