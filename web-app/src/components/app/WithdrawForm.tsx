"use client";

import { useEffect, useState } from "react";
import { EXIT_FEE_BPS, VAULT_META, type Ticker } from "@/constants/vaults";
import { redeem, requestExit } from "@/lib/chain/actions";
import { fmt } from "@/lib/format";
import { useRefresh } from "@/lib/use-refresh";
import { useSigner } from "@/lib/use-signer";
import { redemptionPreview } from "@/lib/yield";
import { usePositionStore } from "@/store/position-provider";
import { useUiStore } from "@/store/ui-provider";
import { useVaultStore } from "@/store/vault-provider";
import { useWalletStore } from "@/store/wallet-provider";

const clean = (s: string) => s.replace(/[^0-9.]/g, "").replace(/(\..*)\./g, "$1");

export default function WithdrawForm({ t, onConnect }: { t: Ticker; onConnect: () => void }) {
  const v = useVaultStore((s) => s.vaults[t]);
  const p = usePositionStore((s) => s.positions[t]);
  const e = usePositionStore((s) => s.pendingExits[t]);
  const status = useWalletStore((s) => s.status);
  const setTab = useUiStore((s) => s.setTab);
  const showToast = useUiStore((s) => s.showToast);
  const refresh = useRefresh();
  const signer = useSigner();
  const [amt, setAmt] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const meta = VAULT_META[t];
  const connected = status === "connected";

  useEffect(() => {
    if (e.shares <= 0 || e.ready) return;
    const i = setInterval(() => {
      setNow(Date.now());
      if (Date.now() >= e.readyAt) refresh();
    }, 1000);
    return () => clearInterval(i);
  }, [e.shares, e.ready, e.readyAt, refresh]);

  async function claim() {
    setBusy(true);
    try {
      const out = await redeem(t, signer, BigInt(e.nonce));
      showToast(`Claimed ${fmt(out.stock)} ${meta.token} and ${fmt(out.usdc)} USDC to your wallet.`);
      setTab("deposit");
      refresh();
    } catch (er) {
      showToast(er instanceof Error ? er.message : "Claim failed.");
    } finally {
      setBusy(false);
    }
  }

  if (e.shares > 0) {
    const preview = redemptionPreview(e.stockAmount, e.usdcAmount, v.priceUsd, EXIT_FEE_BPS);
    if (e.ready)
      return (
        <>
          <p style={{ fontSize: 13, color: "var(--grey)" }}>Ready to claim</p>
          <p className="big num">
            {fmt(preview.stockOut)} {meta.token}
          </p>
          <p className="serif" style={{ fontSize: 24, margin: "4px 0 18px" }}>
            plus {fmt(preview.usdcOut)} USDC
          </p>
          {preview.stockReduced && (
            <p className="fine" style={{ margin: "0 0 14px" }}>
              This vault is young and hasn&apos;t earned back its set-up costs yet, so the USDC leg is zero and a sliver of stock covers the difference.
            </p>
          )}
          <button className="btn r" onClick={claim} disabled={busy}>
            {busy ? "Claiming…" : "Claim to wallet"}
          </button>
        </>
      );
    const left = Math.max(0, Math.ceil((e.readyAt - now) / 1000));
    return (
      <>
        <p style={{ fontSize: 13, color: "var(--grey)" }}>Withdrawal requested</p>
        <p className="big num">
          {fmt(e.stockAmount)} {meta.token}
        </p>
        <div className="steps">
          <div className="done">
            <i />
            <span>
              <b>Requested</b>Your stock keeps earning until it settles
            </span>
          </div>
          <div className="now">
            <i />
            <span>
              <b>Settling at the top of the hour</b>
              <span>Ready in about {left}s{process.env.NEXT_PUBLIC_DATA_SOURCE === "rpc" ? "" : " (demo clock)"}</span>
            </span>
          </div>
          <div>
            <i />
            <span>
              <b>Ready to claim</b>
              {fmt(preview.stockOut)} {meta.token} plus {fmt(preview.usdcOut)} USDC
            </span>
          </div>
        </div>
      </>
    );
  }

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

  const a = parseFloat(amt) || 0;
  const frac = p.stockAmount > 0 ? Math.min(1, a / p.stockAmount) : 0;
  const preview = redemptionPreview(Math.min(a, p.stockAmount), p.usdcEarned * frac, v.priceUsd, 0);
  const tooMuch = a > p.stockAmount + 1e-9;

  async function go() {
    if (a <= 0) return setErr("Enter how much to withdraw.");
    if (tooMuch) return setErr(`You have ${fmt(p.stockAmount)} ${meta.token} in this vault.`);
    setErr("");
    setBusy(true);
    try {
      await requestExit(t, a, signer);
      showToast(`Withdrawal requested. ${fmt(a)} ${meta.token} will be ready at the top of the hour.`);
      setAmt("");
      refresh();
    } catch (er) {
      setErr(er instanceof Error ? er.message : "Withdrawal failed.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <label htmlFor="amt">You withdraw</label>
      <div className="field">
        <input id="amt" inputMode="decimal" autoComplete="off" placeholder="0.00" value={amt} onChange={(ev) => setAmt(clean(ev.target.value))} />
        <span className="u">{meta.token}</span>
        <button className="mx" onClick={() => setAmt(String(p.stockAmount))}>
          All
        </button>
      </div>
      <div className="bal">
        <span>
          Deposited: {fmt(p.stockAmount)} {meta.token}
        </span>
        <span>
          Earned: <span className="num">{fmt(p.usdcEarned)}</span> USDC
        </span>
      </div>
      <div className="err" role="alert">
        {err || (tooMuch ? `You have ${fmt(p.stockAmount)} ${meta.token} in this vault.` : "")}
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
