import { existsSync } from "node:fs";
import path from "node:path";
import Earned from "@/components/landing/Earned";
import Footer from "@/components/landing/Footer";
import Hero from "@/components/landing/Hero";
import LandingBar from "@/components/landing/LandingBar";
import Laps from "@/components/landing/Laps";
import LightsOut from "@/components/landing/LightsOut";
import LiveGrid from "@/components/landing/LiveGrid";
import Trust from "@/components/landing/Trust";

export default function LandingPage() {
  // Server component: only ship the <video> when the footage file is actually in public/.
  const hasFootage = existsSync(path.join(process.cwd(), "public", "hero.mp4"));
  const showSlotTag = process.env.NEXT_PUBLIC_SHOW_SLOT_TAG === "1";
  return (
    <>
      <LandingBar />
      <Hero hasFootage={hasFootage} showSlotTag={showSlotTag} />
      <LiveGrid />
      <Laps />
      <Earned />
      <Trust />
      <LightsOut />
      <Footer />
    </>
  );
}
