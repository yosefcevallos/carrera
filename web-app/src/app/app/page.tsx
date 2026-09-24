import { Suspense } from "react";
import type { Metadata } from "next";
import AppBar from "@/components/app/AppBar";
import AppMain from "@/components/app/AppMain";

export const metadata: Metadata = { title: "Carrera app — Vaults" };

export default function AppPage() {
  return (
    <>
      <AppBar />
      <Suspense>
        <AppMain />
      </Suspense>
    </>
  );
}
