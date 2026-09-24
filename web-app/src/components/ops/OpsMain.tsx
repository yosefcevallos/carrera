"use client";

import { DATA_SOURCE, OPS_ALLOWED_WALLETS } from "@/lib/chain/config";
import { useOpsStore } from "@/store/ops-provider";
import { useWalletStore } from "@/store/wallet-provider";
import BookList from "./BookList";
import BookView from "./BookView";
import FundingChart from "./FundingChart";
import KeeperPanel from "./KeeperPanel";
import RuleAndHealth from "./RuleAndHealth";

export default function OpsMain() {
  const address = useWalletStore((s) => s.address);
  const selected = useOpsStore((s) => s.selected);
  const allowed = DATA_SOURCE === "mock" || (address !== "" && OPS_ALLOWED_WALLETS.includes(address));

  if (!allowed) {
    return (
      <main className="ops-gate">
        <p className="serif" style={{ fontSize: 30 }}>
          Pit lane is closed.
        </p>
        <p>{address ? "This wallet is not on the ops list." : "Connect the keeper or guardian wallet to open the monitor."}</p>
      </main>
    );
  }

  return (
    <main className="ops">
      <BookList />
      <section aria-label={`${selected} book`}>
        <BookView t={selected} />
        <RuleAndHealth t={selected} />
        <FundingChart t={selected} />
        <KeeperPanel />
      </section>
    </main>
  );
}
