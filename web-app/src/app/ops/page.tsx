import type { Metadata } from "next";
import OpsSyncer from "@/components/OpsSyncer";
import AppBar from "@/components/app/AppBar";
import OpsMain from "@/components/ops/OpsMain";

export const metadata: Metadata = { title: "Carrera ops — Books" };

export default function OpsPage() {
  return (
    <>
      <AppBar />
      <OpsSyncer />
      <OpsMain />
    </>
  );
}
