"use client";

import { useState } from "react";
import { VAULT_META, type Ticker } from "@/constants/vaults";
import { deposit } from "@/lib/chain/actions";
import { fmt, usd } from "@/lib/format";
import { useRefresh } from "@/lib/use-refresh";
import { useSigner } from "@/lib/use-signer";
import { usePositionStore } from "@/store/position-provider";
import { useUiStore } from "@/store/ui-provider";
import { useVaultStore } from "@/store/vault-provider";
import { useWalletStore } from "@/store/wallet-provider";

const clean = (s: string) => s.replace(/[^0-9.]/g, "").replace(/(\..*)\./g, "$1");

export default function DepositForm({ t, onConnect }: { t: Ticker; onConnect: () => void }) {
  const v = useVaultStore((s) => s.vaults[t]);
  const bal = usePositionStore((s) => s.balances[t]);
  const status = useWalletStore((s) => s.status);
  const setTab = useUiStore((s) => s.setTab);
  const showToast = useUiStore((s) => s.showToast);
  const refresh = useRefresh();
  const signer = useSigner();
  const [amt, setAmt] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const meta = VAULT_META[t];
  const connected = status === "connected";
  const a = parseFloat(amt) || 0;
  const tooMuch = connected && a > bal;

  async function go() {
    if (!connected) return onConnect();
    if (a <= 0) return setErr("Enter how much to deposit.");
    if (tooMuch) return setErr(`That's more than the ${fmt(bal)} ${meta.token} in your wallet.`);
    setErr("");
    setBusy(true);
    try {
      await deposit(t, a, signer);
      showToast(`Deposited ${fmt(a)} ${meta.token}. You keep every move in ${meta.name}'s price.`);
      setAmt("");
      setTab("withdraw");
      refresh(); // background; the toast fires first
    } catch (e) {
      setErr(e instanceof Error ? e.message : "Deposit failed.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <label htmlFor="amt">You deposit</label>
      <div className="field">
        <input id="amt" inputMode="decimal" autoComplete="off" placeholder="0.00" value={amt} onChange={(e) => setAmt(clean(e.target.value))} />
        <span className="u">{meta.token}</span>
        <button className="mx" onClick={() => (connected ? setAmt(String(bal)) : onConnect())}>
          Max
        </button>
      </div>
      <div className="bal">
        <span>{connected ? `In your wallet: ${fmt(bal)} ${meta.token}` : "Connect a wallet to see your balance"}</span>
        <span className="num">{a ? `≈ ${usd(a * v.priceUsd)}` : ""}</span>
      </div>
      <div className="err" role="alert">
        {err || (tooMuch ? `That's more than the ${fmt(bal)} ${meta.token} in your wallet.` : "")}
      </div>
      {v.mode !== "funding" && (
        <div className="note g">
          <i />
          <p>
            <b>{v.mode === "parked" ? "Parked for now" : "Idle for now"}</b>
            <span>
              Funding on {meta.name} is below its hurdle, so {v.mode === "parked" ? "the vault's USDC is supplied on Kamino" : "the vault has repaid its loan and earns nothing"}. It goes back to funding on its own
              when the 24-hour average clears the entry level.
            </span>
          </p>
        </div>
      )}
      <dl className="two">
        <div>
          <dt>You keep</dt>
          <dd>100%</dd>
          <small>of {meta.name}&apos;s price moves</small>
        </div>
        <div>
          <dt>Shares you get</dt>
          <dd className="num">{a ? fmt((a * v.priceUsd) / Math.max(1e-9, v.priceUsd + v.usdcPerShare), 4) : "—"}</dd>
          <small>1 share = 1 {meta.token} + {fmt(v.usdcPerShare, 3)} USDC today</small>
        </div>
      </dl>
      <button className="btn" onClick={go} disabled={busy}>
        {connected ? (busy ? "Confirming…" : `Deposit ${meta.token}`) : "Connect wallet to deposit"}
      </button>
    </>
  );
}
