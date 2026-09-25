"use client";

import { useState } from "react";
import { VAULT_META, type Ticker } from "@/constants/vaults";
import { requestExit } from "@/lib/chain/actions";
import { formatRaw, parseToRaw, rawToNumber } from "@/lib/amount";
import { fmt } from "@/lib/format";
import { useRefresh } from "@/lib/use-refresh";
import { useSigner } from "@/lib/use-signer";
import { readyAtFor } from "@/lib/exits";
import { redemptionPreview } from "@/lib/yield";
import { usePositionStore } from "@/store/position-provider";
import { useUiStore } from "@/store/ui-provider";
import { useVaultStore } from "@/store/vault-provider";
import { useWalletStore } from "@/store/wallet-provider";

const clean = (s: string) => s.replace(/[^0-9.]/g, "").replace(/(\..*)\./g, "$1");

export default function WithdrawForm({ t, onConnect }: { t: Ticker; onConnect: () => void }) {
  const v = useVaultStore((s) => s.vaults[t]);
  const p = usePositionStore((s) => s.positions[t]);
  const sharesRawStr = usePositionStore((s) => s.sharesRaw[t]);
  const decimals = usePositionStore((s) => s.decimals[t]);
  const status = useWalletStore((s) => s.status);
  const setTab = useUiStore((s) => s.setTab);
  const addExit = usePositionStore((s) => s.addExit);
  const showToast = useUiStore((s) => s.showToast);
  const refresh = useRefresh();
  const signer = useSigner();
  const [amt, setAmt] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const [stale, setStale] = useState(false);
  const meta = VAULT_META[t];
  const connected = status === "connected";

  if (!connected || p.shares <= 0)
    return (
      <>
        <div className="note g">
          <i />
          <p>
            <b>Nothing to withdraw yet</b>
            <span>{connected ? `You don't have ${meta.token} in this vault.` : "Connect a wallet to see your deposits."}</span>
          </p>
        </div>
        <button className="btn" onClick={() => (connected ? setTab("deposit") : onConnect())}>
          {connected ? `Deposit ${meta.token}` : "Connect wallet"}
        </button>
      </>
    );

  // The input is in stock units; one share = one deposited stock unit in the rpc position mapping,
  // so the typed amount parses straight to raw shares. Exact integer math on base units.
  const sharesRaw = BigInt(sharesRawStr);
  const raw = parseToRaw(amt, decimals);
  const a = rawToNumber(raw, decimals);
  const frac = p.stockAmount > 0 ? Math.min(1, a / p.stockAmount) : 0;
  const preview = redemptionPreview(Math.min(a, p.stockAmount), p.usdcEarned * frac, v.priceUsd, 0);
  const tooMuch = raw > sharesRaw;

  async function go() {
    if (raw <= 0n) return setErr("Enter how much to withdraw.");
    if (tooMuch) return setErr(`You have ${fmt(p.stockAmount, 4)} ${meta.token} in this vault.`);
    setErr("");
    setBusy(true);
    try {
      const nonce = await requestExit(t, { raw, ui: a }, signer);
      addExit(t, { nonce, shares: a, stockAmount: a, usdcAmount: 0, epochId: 0, status: "open", requestedAt: Date.now(), readyAt: readyAtFor(Date.now()) });
      showToast(`Withdrawal requested. ${fmt(a, 4)} ${meta.token} will be ready at the top of the hour.`);
      setAmt("");
      setTab("requests");
    } catch (er) {
      setErr(er instanceof Error ? er.message : "Withdrawal failed.");
    } finally {
      setStale(true);
      setBusy(false);
      await refresh();
      setStale(false);
    }
  }

  return (
    <>
      <label htmlFor="amt">You withdraw</label>
      <div className="field">
        <input id="amt" inputMode="decimal" autoComplete="off" placeholder="0.00" value={amt} onChange={(ev) => setAmt(clean(ev.target.value))} />
        <span className="u">{meta.token}</span>
        <button className="mx" onClick={() => setAmt(formatRaw(sharesRaw, decimals))}>
          All
        </button>
      </div>
      <div className="bal">
        <span>
          Deposited: {stale ? "updating…" : `${fmt(p.stockAmount, 4)} ${meta.token}`}
        </span>
        <span>
          Earned: <span className="num">{fmt(p.usdcEarned)}</span> USDC
        </span>
      </div>
      <div className="err" role="alert">
        {err || (tooMuch ? `You have ${fmt(p.stockAmount, 4)} ${meta.token} in this vault.` : "")}
      </div>
      <dl className="two">
        <div>
          <dt>You get back</dt>
          <dd className="num">{a ? `${fmt(preview.stockOut, preview.stockReduced ? 4 : 2)} ${meta.token}` : "—"}</dd>
          <small>{preview.stockReduced && a ? "slightly less than deposited, see below" : "of your deposit"}</small>
        </div>
        <div>
          <dt>Plus</dt>
          <dd className="red num">{a ? fmt(preview.usdcOut) : "—"}</dd>
          <small>USDC, after the 15% fee</small>
        </div>
      </dl>
      {preview.stockReduced && a > 0 && (
        <div className="note g">
          <i />
          <p>
            <b>No USDC yet</b>
            <span>This vault is young and hasn&apos;t earned back its set-up costs, so a sliver of stock covers the shortfall instead. It turns positive as funding accrues.</span>
          </p>
        </div>
      )}
      <div className="note">
        <i />
        <p>
          <b>Ready within the hour</b>
          <span>Withdrawals settle at the top of each hour. Your stock keeps earning until then.</span>
        </p>
      </div>
      <button className="btn" onClick={go} disabled={busy}>
        {busy ? "Confirming…" : "Withdraw"}
      </button>
    </>
  );
}
