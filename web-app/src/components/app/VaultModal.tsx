"use client";

import { useEffect, useRef } from "react";
import { useWalletModal } from "@solana/wallet-adapter-react-ui";
import { VAULT_META, PERF_FEE_BPS, EXIT_FEE_BPS, type Ticker } from "@/constants/vaults";
import Roundel from "@/components/Roundel";
import { DATA_SOURCE } from "@/lib/chain/config";
import { DEMO_WALLET } from "@/lib/mock/world";
import { pct } from "@/lib/format";
import { modeLong } from "@/lib/yield";
import { useUiStore } from "@/store/ui-provider";
import { useVaultStore } from "@/store/vault-provider";
import { useWalletStore } from "@/store/wallet-provider";
import DepositForm from "./DepositForm";
import SharePriceChart from "./SharePriceChart";
import WithdrawForm from "./WithdrawForm";

export default function VaultModal({ t, returnFocus }: { t: Ticker; returnFocus: () => void }) {
  const v = useVaultStore((s) => s.vaults[t]);
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
            <Roundel n={meta.roundel} />
            <span className="pill">
              <i />
              {modeLong[v.mode]}
            </span>
            <button className="x" onClick={close} aria-label="Close">
              ×
            </button>
          </div>
          <div>
            <div className="tk" id="md-tk">
              {t}
            </div>
            <div className="co">{meta.name}</div>
          </div>
        </div>
        <div className="md-grid">
          <div className="md-chart">
            <SharePriceChart t={t} />
          </div>
          <div className="md-side">
            <div className="seg" role="tablist">
              <button role="tab" aria-selected={tab === "deposit"} onClick={() => setTab("deposit")}>
                Deposit
              </button>
              <button role="tab" aria-selected={tab === "withdraw"} onClick={() => setTab("withdraw")}>
                Withdraw
              </button>
            </div>
            <div className="md-b">
              {tab === "deposit" ? <DepositForm t={t} onConnect={onConnect} /> : <WithdrawForm t={t} onConnect={onConnect} />}
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
                    <b>Closes the trade and supplies the USDC on Kamino, or repays the loan if supply pays less than borrow</b>
                  </div>
                  <div>
                    <span>Switches when</span>
                    <b>
                      24h funding above {pct(v.hurdleBps + v.enterMarginBps)} to enter, below {pct(v.hurdleBps - v.exitMarginBps)} to leave
                    </b>
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
      </div>
    </div>
  );
}
