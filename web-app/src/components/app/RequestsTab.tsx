"use client";

import { useEffect, useState } from "react";
import { EXIT_FEE_BPS, VAULT_META, type Ticker } from "@/constants/vaults";
import { cancelExit, redeem } from "@/lib/chain/actions";
import { fmt } from "@/lib/format";
import { useRefresh } from "@/lib/use-refresh";
import { useSigner } from "@/lib/use-signer";
import { redemptionPreview } from "@/lib/yield";
import type { VaultExit } from "@/lib/types";
import { usePositionStore } from "@/store/position-provider";
import { useUiStore } from "@/store/ui-provider";
import { useVaultStore } from "@/store/vault-provider";
import { useWalletStore } from "@/store/wallet-provider";

const when = (ms: number) => new Date(ms).toLocaleString(undefined, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });

export default function RequestsTab({ t, onConnect }: { t: Ticker; onConnect: () => void }) {
  const v = useVaultStore((s) => s.vaults[t]);
  const exits = usePositionStore((s) => s.exits[t]);
  const setExitStatus = usePositionStore((s) => s.setExitStatus);
  const status = useWalletStore((s) => s.status);
  const showToast = useUiStore((s) => s.showToast);
  const setTab = useUiStore((s) => s.setTab);
  const refresh = useRefresh();
  const signer = useSigner();
  const [busy, setBusy] = useState("");
  const [now, setNow] = useState(() => Date.now());
  const meta = VAULT_META[t];
  const connected = status === "connected";
  const settling = exits.some((e) => e.status === "open");

  // Tick while something is settling so countdowns move and the mock flips to ready.
  useEffect(() => {
    if (!settling) return;
    const i = setInterval(() => {
      setNow(Date.now());
      if (exits.some((e) => e.status === "open" && Date.now() >= e.readyAt)) refresh();
    }, 1000);
    return () => clearInterval(i);
  }, [settling, exits, refresh]);

  async function claim(e: VaultExit) {
    setBusy(e.nonce);
    try {
      const out = await redeem(t, e.nonce, signer);
      setExitStatus(t, e.nonce, "redeemed"); // card, table cell, Action button and badge flip in this render
      const stock = out.stock || e.stockAmount;
      const usdc = out.usdc || e.usdcAmount;
      showToast(`Claimed ${fmt(stock, 4)} ${meta.token} and ${fmt(usdc)} USDC to your wallet.`);
    } catch (er) {
      showToast(er instanceof Error ? er.message : "Claim failed.");
    } finally {
      setBusy("");
      await refresh();
    }
  }

  async function cancel(e: VaultExit) {
    setBusy(e.nonce);
    try {
      await cancelExit(t, e.nonce, signer);
      setExitStatus(t, e.nonce, "cancelled");
      showToast(`Cancelled the withdrawal of ${fmt(e.shares, 4)} ${meta.token}. It stays in the vault.`);
    } catch (er) {
      showToast(er instanceof Error ? er.message : "Cancel failed.");
    } finally {
      setBusy("");
      await refresh();
    }
  }

  if (!connected)
    return (
      <>
        <div className="note g">
          <i />
          <p>
            <b>No withdrawal requests yet.</b>
            <span>Connect a wallet to see your requests.</span>
          </p>
        </div>
        <button className="btn" onClick={onConnect}>
          Connect wallet
        </button>
      </>
    );

  if (exits.length === 0)
    return (
      <>
        <div className="note g">
          <i />
          <p>
            <b>No withdrawal requests yet.</b>
            <span>Withdrawals you request show up here while they settle, and stay listed once claimed.</span>
          </p>
        </div>
        <button className="btn" onClick={() => setTab("withdraw")}>
          Withdraw {meta.token}
        </button>
      </>
    );

  return (
    <div className="reqs">
      {exits.map((e) => {
        const preview = redemptionPreview(e.stockAmount, e.usdcAmount, v.priceUsd, e.status === "settled" ? EXIT_FEE_BPS : 0);
        const done = e.status === "redeemed" || e.status === "cancelled";
        const left = Math.max(0, Math.ceil((e.readyAt - now) / 1000));
        const cancellable = e.status === "open" && now < e.readyAt;
        return (
          <div className={`req${done ? " done" : ""}`} key={e.nonce}>
            <div className="req-h">
              <b className="num">
                {fmt(e.shares, 4)} {meta.token}
              </b>
              <span>{when(e.requestedAt)}</span>
            </div>
            {e.status === "redeemed" && (
              <p className="req-done">
                Claimed {fmt(e.stockAmount, 4)} {meta.token} plus {fmt(e.usdcAmount)} USDC
              </p>
            )}
            {e.status === "cancelled" && <p className="req-done">Cancelled, shares returned to the vault</p>}
            {!done && (
              <div className="steps">
                <div className="done">
                  <i />
                  <span>
                    <b>Requested</b>Your stock keeps earning until it settles
                  </span>
                </div>
                <div className={e.status === "open" ? "now" : "done"}>
                  <i />
                  <span>
                    <b>Settling at the top of the hour</b>
                    {e.status === "open" && (
                      <span>
                        {left > 0 ? `Ready in about ${left >= 60 ? `${Math.ceil(left / 60)} min` : `${left}s`}` : "Waiting for the keeper to settle this epoch"}
                        {process.env.NEXT_PUBLIC_DATA_SOURCE === "rpc" ? "" : " (demo clock)"}
                      </span>
                    )}
                  </span>
                </div>
                <div className={e.status === "settled" ? "now" : ""}>
                  <i />
                  <span>
                    <b>Ready to claim</b>
                    {fmt(preview.stockOut, 4)} {meta.token} plus {fmt(preview.usdcOut)} USDC
                    {preview.stockReduced ? ", USDC leg is zero so a sliver of stock covers the set-up cost" : ""}
                  </span>
                </div>
              </div>
            )}
            {e.status === "settled" && (
              <button className="btn r" onClick={() => claim(e)} disabled={busy === e.nonce}>
                {busy === e.nonce ? "Claiming…" : "Claim to wallet"}
              </button>
            )}
            {cancellable && (
              <button className="req-cancel" onClick={() => cancel(e)} disabled={busy === e.nonce}>
                {busy === e.nonce ? "Cancelling…" : "Cancel this request"}
              </button>
            )}
          </div>
        );
      })}
    </div>
  );
}
