"use client";

import { useState } from "react";
import { VAULT_META, type Ticker } from "@/constants/vaults";
import { deposit } from "@/lib/chain/actions";
import { formatRaw, parseToRaw, rawToNumber } from "@/lib/amount";
import { fmt, usd } from "@/lib/format";
import { useRefresh } from "@/lib/use-refresh";
import { useSigner } from "@/lib/use-signer";
import { currentApyBps } from "@/lib/yield";
import { usePositionStore } from "@/store/position-provider";
import { useUiStore } from "@/store/ui-provider";
import { useVaultStore } from "@/store/vault-provider";
import { useWalletStore } from "@/store/wallet-provider";

const clean = (s: string) => s.replace(/[^0-9.]/g, "").replace(/(\..*)\./g, "$1");

export default function DepositForm({ t, onConnect }: { t: Ticker; onConnect: () => void }) {
  const v = useVaultStore((s) => s.vaults[t]);
  const balRawStr = usePositionStore((s) => s.balancesRaw[t]);
  const decimals = usePositionStore((s) => s.decimals[t]);
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
  // Everything that reaches the chain is exact integer math on base units.
  const balRaw = BigInt(balRawStr);
  const bal = rawToNumber(balRaw, decimals);
  const raw = parseToRaw(amt, decimals);
  const a = rawToNumber(raw, decimals);
  const tooMuch = connected && raw > balRaw;
  const yieldPct = currentApyBps(v) / 100;

  async function go() {
    if (!connected) return onConnect();
    if (raw <= 0n) return setErr("Enter how much to deposit.");
    if (tooMuch) return setErr(`That's more than the ${fmt(bal, 4)} ${meta.token} in your wallet.`);
    setErr("");
    setBusy(true);
    try {
      await deposit(t, { raw, ui: a }, signer);
      showToast(`Deposited ${fmt(a, 4)} ${meta.token}. You keep every move in ${meta.name}'s price.`);
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
        <button className="mx" onClick={() => (connected ? setAmt(formatRaw(balRaw, decimals)) : onConnect())}>
          Max
        </button>
      </div>
      <div className="bal">
        <span>{connected ? `In your wallet: ${fmt(bal, 4)} ${meta.token}` : "Connect a wallet to see your balance"}</span>
        <span className="num">{raw > 0n ? `≈ ${usd(a * v.priceUsd)}` : ""}</span>
      </div>
      <div className="err" role="alert">
        {err || (tooMuch ? `That's more than the ${fmt(bal, 4)} ${meta.token} in your wallet.` : "")}
      </div>
      {v.mode === "parked" && (
        <div className="note g">
          <i />
          <p>
            <b>Earning from lending for now</b>
            <span>Funding on {meta.name} is low, so the vault lends its USDC on Kamino. It switches back to funding on its own when rates pick up.</span>
          </p>
        </div>
      )}
      {v.mode === "idle" && (
        <div className="note g">
          <i />
          <p>
            <b>Waiting for funding</b>
            <span>
              Funding on {meta.name} is below the hurdle and Kamino&apos;s supply rate is below its borrow rate, so the loan is repaid for now. It switches on by itself
              when rates allow.
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
          <dt>You earn, est.</dt>
          <dd className="red num">{raw > 0n ? `${usd((a * v.priceUsd * yieldPct) / 100)} a year` : "—"}</dd>
          <small>in USDC, about {fmt(yieldPct, 1)}%</small>
        </div>
      </dl>
      <button className="btn" onClick={go} disabled={busy}>
        {connected ? (busy ? "Confirming…" : `Deposit ${meta.token}`) : "Connect wallet to deposit"}
      </button>
    </>
  );
}
