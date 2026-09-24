import Earned from "@/components/landing/Earned";
import Footer from "@/components/landing/Footer";
import Hero from "@/components/landing/Hero";
import LandingBar from "@/components/landing/LandingBar";
import Laps from "@/components/landing/Laps";
import LightsOut from "@/components/landing/LightsOut";
import Trust from "@/components/landing/Trust";

export default function LandingPage() {
  return (
    <>
      <LandingBar />
      <Hero />
      <Laps />
      <Earned />
      <Trust />
      <LightsOut />
      <Footer />
    </>
  );
}
