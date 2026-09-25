"use client";

import { useEffect, useState } from "react";
import { EXIT_FEE_BPS, VAULT_META, type Ticker } from "@/constants/vaults";
import { cancelExit, redeem } from "@/lib/chain/actions";
import { formatCountdown, hourUtc } from "@/lib/app2";
import { fmt } from "@/lib/format";
import { useRefresh } from "@/lib/use-refresh";
import { useSigner } from "@/lib/use-signer";
import { redemptionPreview } from "@/lib/yield";
import type { VaultExit } from "@/lib/types";
import { usePositionStore } from "@/store/position-provider";
import { useUiStore } from "@/store/ui-provider";
import { useVaultStore } from "@/store/vault-provider";
import { useWalletStore } from "@/store/wallet-provider";
import Receipt, { clockText } from "./Receipt";

const when = (ms: number) => new Date(ms).toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", hour12: false });

interface Done {
  stock: number;
  usdc: number;
  signedAt: number;
  confirmedAt: number;
  signature: string;
}

export default function RequestsTab({ t, onConnect }: { t: Ticker; onConnect: () => void }) {
  const v = useVaultStore((s) => s.vaults[t]);
  const exits = usePositionStore((s) => s.exits[t]);
  const setExitStatus = usePositionStore((s) => s.setExitStatus);
  const status = useWalletStore((s) => s.status);
  const showToast = useUiStore((s) => s.showToast);
  const setTab = useUiStore((s) => s.setTab);
  const close = useUiStore((s) => s.closeVaultWindow);
  const refresh = useRefresh();
  const signer = useSigner();
  const [busy, setBusy] = useState("");
  const [done, setDone] = useState<Done | null>(null);
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
    const signedAt = Date.now();
    try {
      const out = await redeem(t, e.nonce, signer);
      setExitStatus(t, e.nonce, "redeemed"); // card, table cell and badge flip in this render
      const stock = out.stock || e.stockAmount;
      const usdc = out.usdc || e.usdcAmount;
      setDone({ stock, usdc, signedAt, confirmedAt: Date.now(), signature: out.signature });
      showToast(`Claimed ${fmt(stock, 4)} ${meta.token}.`);
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
      showToast(`Cancelled the withdrawal of ${fmt(e.shares, 4)} ${meta.token}.`);
    } catch (er) {
      showToast(er instanceof Error ? er.message : "Cancel failed.");
    } finally {
      setBusy("");
      await refresh();
    }
  }

  if (done)
    return (
      <Receipt
        title="Claim confirmed"
        blurb={`${fmt(done.stock, 4)} ${meta.token} and ${fmt(done.usdc, 2)} USDC are back in your wallet.`}
        timeline={[
          { label: "Signed in wallet", time: clockText(done.signedAt), state: "done" },
          { label: "Confirmed on Solana", time: clockText(done.confirmedAt), state: "done" },
          { label: "Tokens in wallet", time: "now", state: "now" },
        ]}
        rows={[
          { k: "Stock returned", v: `${fmt(done.stock, 4)} ${meta.token}` },
          { k: "USDC earned", v: `${fmt(done.usdc, 2)} USDC` },
        ]}
        signature={done.signature}
        onDone={close}
      />
    );

  if (!connected)
    return (
      <>
        <p className="tnote">Connect a wallet to see your withdrawal requests.</p>
        <button className="go ghost" onClick={onConnect}>
          Connect wallet
        </button>
      </>
    );

  if (exits.length === 0)
    return (
      <>
        <p className="tnote">No withdrawal requests yet. Requests show up here while they settle, and stay listed once claimed.</p>
        <button className="go ghost" onClick={() => setTab("withdraw")}>
          Withdraw {meta.token}
        </button>
      </>
    );

  return (
    <div className="reqs2">
      {exits.map((e) => {
        const preview = redemptionPreview(e.stockAmount, e.usdcAmount, v.priceUsd, e.status === "settled" ? EXIT_FEE_BPS : 0);
        const finished = e.status === "redeemed" || e.status === "cancelled";
        const left = e.readyAt - now;
        const cancellable = e.status === "open" && now < e.readyAt;
        return (
          <div className={`req2${finished ? " done" : ""}`} key={e.nonce}>
            <div className="lbl">
              <b className="mono">
                {fmt(e.shares, 4)} {meta.token}
              </b>
              <span className="mono">{when(e.requestedAt)}</span>
            </div>
            {e.status === "redeemed" && (
              <p className="tnote">
                Claimed {fmt(e.stockAmount, 4)} {meta.token} plus {fmt(e.usdcAmount, 2)} USDC
              </p>
            )}
            {e.status === "cancelled" && <p className="tnote">Cancelled, shares returned to the vault</p>}
            {!finished && (
              <div className="tl">
                <div className="d">
                  <i />
                  <span>Requested</span>
                  <em className="mono">{clockText(e.requestedAt)}</em>
                </div>
                <div className={e.status === "open" ? "n" : "d"}>
                  <i />
                  <span>Settles at {hourUtc(e.readyAt)}</span>
                  <em className="mono">{e.status === "open" ? (left > 0 ? `in ${formatCountdown(left)}` : "settling…") : "done"}</em>
                </div>
                <div className={e.status === "settled" ? "n" : ""}>
                  <i />
                  <span>Ready to claim</span>
                  <em className="mono">
                    {fmt(preview.stockOut, 4)} {meta.token} + {fmt(preview.usdcOut, 2)} USDC
                  </em>
                </div>
              </div>
            )}
            {e.status === "settled" && (
              <button className="go" onClick={() => claim(e)} disabled={busy === e.nonce}>
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
