import Link from "next/link";
import Mark from "@/components/Mark";
import WalletChip from "./WalletChip";

export default function AppBar() {
  return (
    <header className="bar">
      <nav className="l" aria-label="App">
        <Link href="/app">Vaults</Link>
        <Link className="dim" href="/ops">
          Ops
        </Link>
        <Link className="dim" href="/">
          About
        </Link>
      </nav>
      <Mark />
      <div className="r">
        <WalletChip />
      </div>
    </header>
  );
}
