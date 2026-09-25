"use client";

import { useEffect, useRef } from "react";
import { useWalletModal } from "@solana/wallet-adapter-react-ui";
import { VAULT_META, PERF_FEE_BPS, EXIT_FEE_BPS, type Ticker } from "@/constants/vaults";
import TokenIcon from "@/components/TokenIcon";
import { DATA_SOURCE } from "@/lib/chain/config";
import { DEMO_WALLET } from "@/lib/mock/world";
import { pct } from "@/lib/format";
import { formatGrowth, modeLong, realisedGrowth } from "@/lib/yield";
import { useUiStore } from "@/store/ui-provider";
import { useVaultStore } from "@/store/vault-provider";
import { useWalletStore } from "@/store/wallet-provider";
import { requestsTabLabel } from "@/lib/exits";
import { usePositionStore } from "@/store/position-provider";
import DepositForm from "./DepositForm";
import RequestsTab from "./RequestsTab";
import WithdrawForm from "./WithdrawForm";

/**
 * Single-column vault window: header band, Deposit / Withdraw tabs, the form, and a collapsed
 * "How this vault works". No chart. Closes on ×, Escape or a scrim click; focus moves in on open
 * and back to the row on close.
 */
export default function VaultModal({ t, returnFocus }: { t: Ticker; returnFocus: () => void }) {
  const v = useVaultStore((s) => s.vaults[t]);
  const exits = usePositionStore((s) => s.exits[t]);
  const tab = useUiStore((s) => s.tab);
  const setTab = useUiStore((s) => s.setTab);
  const details = useUiStore((s) => s.detailsOpen);
  const toggleDetails = useUiStore((s) => s.toggleDetails);
  const close = useUiStore((s) => s.closeVaultWindow);
  const setWallet = useWalletStore((s) => s.setWallet);
  const showToast = useUiStore((s) => s.showToast);
  const { setVisible } = useWalletModal();
  const dialog = useRef<HTMLDivElement>(null);
  const meta = VAULT_META[t];
  const funding = v.mode === "funding";

  useEffect(() => {
    document.body.style.overflow = "hidden";
    const first = dialog.current?.querySelector<HTMLElement>("input, button");
    first?.focus();
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    document.addEventListener("keydown", key);
    return () => {
      document.body.style.overflow = "";
      document.removeEventListener("keydown", key);
      returnFocus();
    };
  }, [close, returnFocus]);

  function onConnect() {
    if (DATA_SOURCE === "mock") {
      setWallet({ status: "connected", address: DEMO_WALLET, demo: true });
      showToast("No wallet extension is used in demo mode, so you are using a demo wallet.");
    } else setVisible(true);
  }

  return (
    <div className="scrim" onClick={(e) => e.target === e.currentTarget && close()}>
      <div className="md" role="dialog" aria-modal="true" aria-labelledby="md-tk" ref={dialog}>
        <div className={`md-h${funding ? "" : " p"}`}>
          <div className="row">
            <TokenIcon t={t} size={34} eager />
            <span className="pill">
              <i />
              {modeLong[v.mode]}
            </span>
            <button className="x" onClick={close} aria-label="Close">
              ×
            </button>
          </div>
          <div className="tk" id="md-tk">
            {t}
          </div>
          <div className="co">{meta.name}</div>
        </div>
        <div className="seg" role="tablist">
          <button role="tab" aria-selected={tab === "deposit"} onClick={() => setTab("deposit")}>
            Deposit
          </button>
          <button role="tab" aria-selected={tab === "withdraw"} onClick={() => setTab("withdraw")}>
            Withdraw
          </button>
          <button role="tab" aria-selected={tab === "requests"} onClick={() => setTab("requests")}>
            {requestsTabLabel(exits)}
          </button>
        </div>
        <div className="md-b">
          {tab === "deposit" ? <DepositForm t={t} onConnect={onConnect} /> : tab === "withdraw" ? <WithdrawForm t={t} onConnect={onConnect} /> : <RequestsTab t={t} onConnect={onConnect} />}
          <button className="more" aria-expanded={details} onClick={toggleDetails}>
            How this vault works <span>{details ? "Hide" : "Show"}</span>
          </button>
          {details && (
            <div className="det">
              <div>
                <span>What it does</span>
                <b>Borrows USDC against deposits and runs a hedged trade on Phoenix that collects funding</b>
              </div>
              <div>
                <span>When funding is low</span>
                <b>Closes the trade and lends the USDC on Kamino, or repays the loan if lending pays less than borrowing</b>
              </div>
              <div>
                <span>Borrowed against deposits</span>
                <b>{pct(meta.ltvBps, 0)} of value</b>
              </div>
              <div>
                <span>Protected until a stock move of</span>
                <b>
                  {meta.liqBuffer.down}% or +{meta.liqBuffer.up}%
                </b>
              </div>
              <div>
                <span>Realised, 30d</span>
                <b>{(() => { const g = realisedGrowth(v, 30); return g.days > 0 || g.growthPct !== 0 ? formatGrowth(g, 30) : "No history yet"; })()}</b>
              </div>
              <div>
                <span>Rebalanced</span>
                <b>Every minute</b>
              </div>
              <div>
                <span>Fees</span>
                <b>
                  {pct(PERF_FEE_BPS, 0)} of earnings. {pct(EXIT_FEE_BPS, 1)} only if a withdrawal forces an unwind
                </b>
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
