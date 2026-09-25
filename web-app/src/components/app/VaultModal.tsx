"use client";

import { useEffect, useRef } from "react";
import { useWalletModal } from "@solana/wallet-adapter-react-ui";
import { VAULT_META, PERF_FEE_BPS, EXIT_FEE_BPS, type Ticker } from "@/constants/vaults";
import TokenIcon from "@/components/TokenIcon";
import { DATA_SOURCE } from "@/lib/chain/config";
import { DEMO_WALLET } from "@/lib/mock/world";
import { fmt, pct } from "@/lib/format";
import { annualisedPct, currentApyBps, formatGrowth, modeLabel, realisedGrowth } from "@/lib/yield";
import { funding3hPct, fundingNowPct } from "@/lib/app2";
import { useUiStore } from "@/store/ui-provider";
import { useVaultStore } from "@/store/vault-provider";
import { useWalletStore } from "@/store/wallet-provider";
import { requestsTabLabel } from "@/lib/exits";
import { usePositionStore } from "@/store/position-provider";
import DepositForm from "./DepositForm";
import RequestsTab from "./RequestsTab";
import WithdrawForm from "./WithdrawForm";

/**
 * Order ticket (fintech v1): icon, ticker and company, APY with its status dot, three tabs, the
 * ticket body, and a "Vault details" footer that unfolds the how-it-works rows. On phones it is a
 * bottom sheet. Closes on ×, Escape or a scrim click; focus moves in on open and back on close.
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
  const idle = v.vaultState !== 3 && v.vaultState !== 1;
  const apy = currentApyBps(v);

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
      showToast("Demo wallet connected.");
    } else setVisible(true);
  }

  const f3h = funding3hPct(v.fundingSamples, annualisedPct);
  return (
    <div className="scrim" onClick={(e) => e.target === e.currentTarget && close()}>
      <div className="tk" role="dialog" aria-modal="true" aria-labelledby="tk-title" ref={dialog}>
        <div className="grab" aria-hidden="true" />
        <div className="tk-h">
          <TokenIcon t={t} size={30} eager />
          <div>
            <b id="tk-title">{t}</b>
            <small>
              {meta.name} · {meta.token}
            </small>
          </div>
          <div className="ap">
            <div className="mono">{idle ? "Idle" : `${fmt(apy / 100, 2)}%`}</div>
            <span className={idle ? "idle" : ""}>
              <i />
              {modeLabel[v.mode]} · APY
            </span>
            <small className="mono fnow">
              Funding now {fmt(fundingNowPct(v.fundingSamples, annualisedPct), 1)}% · 3h avg{" "}
              {f3h === null ? "n/a" : `${fmt(f3h, 1)}%`} · 24h avg {fmt(v.fundingAvgBps / 100, 1)}%
            </small>
          </div>
          <button className="x" onClick={close} aria-label="Close">
            ×
          </button>
        </div>
        <div className="tabs" role="tablist">
          <button role="tab" className={tab === "deposit" ? "on" : ""} aria-selected={tab === "deposit"} onClick={() => setTab("deposit")}>
            Deposit
          </button>
          <button role="tab" className={tab === "withdraw" ? "on" : ""} aria-selected={tab === "withdraw"} onClick={() => setTab("withdraw")}>
            Withdraw
          </button>
          <button role="tab" className={tab === "requests" ? "on" : ""} aria-selected={tab === "requests"} onClick={() => setTab("requests")}>
            {requestsTabLabel(exits)}
          </button>
        </div>
        <div className="tb">
          {tab === "deposit" ? <DepositForm t={t} onConnect={onConnect} /> : tab === "withdraw" ? <WithdrawForm t={t} onConnect={onConnect} /> : <RequestsTab t={t} onConnect={onConnect} />}
        </div>
        <button className="tf" aria-expanded={details} onClick={toggleDetails}>
          <span>Vault details</span>
          <span>{details ? "×" : "›"}</span>
        </button>
        {details && (
          <div className="det2">
            <div>
              <span>What it does</span>
              <b>Borrows USDC against deposits and runs a hedged trade on Phoenix that collects funding</b>
            </div>
            <div>
              <span>When funding is low</span>
              <b>Closes the trade and supplies the USDC on Kamino, or repays the loan if supply pays less than borrow</b>
            </div>
            <div>
              <span>Borrowed against deposits</span>
              <b className="mono">{pct(v.ltvBps || meta.ltvBps, 0)} of value</b>
            </div>
            <div>
              <span>Protected until a stock move of</span>
              <b className="mono">
                {meta.liqBuffer.down}% or +{meta.liqBuffer.up}%
              </b>
            </div>
            <div>
              <span>Realised, 30d</span>
              <b className="mono">
                {(() => {
                  const g = realisedGrowth(v, 30);
                  return g.days > 0 || g.growthPct !== 0 ? formatGrowth(g, 30) : "No history yet";
                })()}
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
  );
}
