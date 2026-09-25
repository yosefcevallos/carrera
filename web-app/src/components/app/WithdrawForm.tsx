"use client";

import { useEffect, useState } from "react";
import { VAULT_META, type Ticker } from "@/constants/vaults";
import { requestExit } from "@/lib/chain/actions";
import { parseToRaw, rawToNumber } from "@/lib/amount";
import { formatCountdown, hourUtc, nextTopOfHour } from "@/lib/app2";
import { fmt, usd } from "@/lib/format";
import { useRefresh } from "@/lib/use-refresh";
import { useSigner } from "@/lib/use-signer";
import { readyAtFor } from "@/lib/exits";
import { redemptionPreview } from "@/lib/yield";
import { usePositionStore } from "@/store/position-provider";
import { useUiStore } from "@/store/ui-provider";
import { useVaultStore } from "@/store/vault-provider";
import { useWalletStore } from "@/store/wallet-provider";
import AmountBox from "./AmountBox";
import Receipt, { clockText } from "./Receipt";

interface Done {
  amount: number;
  signedAt: number;
  confirmedAt: number;
  signature: string;
  readyAt: number;
}

export default function WithdrawForm({ t, onConnect }: { t: Ticker; onConnect: () => void }) {
  const v = useVaultStore((s) => s.vaults[t]);
  const p = usePositionStore((s) => s.positions[t]);
  const sharesRawStr = usePositionStore((s) => s.sharesRaw[t]);
  const decimals = usePositionStore((s) => s.decimals[t]);
  const status = useWalletStore((s) => s.status);
  const setTab = useUiStore((s) => s.setTab);
  const close = useUiStore((s) => s.closeVaultWindow);
  const addExit = usePositionStore((s) => s.addExit);
  const showToast = useUiStore((s) => s.showToast);
  const refresh = useRefresh();
  const signer = useSigner();
  const [amt, setAmt] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const [stale, setStale] = useState(false);
  const [done, setDone] = useState<Done | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const meta = VAULT_META[t];
  const connected = status === "connected";

  useEffect(() => {
    const i = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(i);
  }, []);

  if (done) {
    return (
      <Receipt
        title="Withdrawal requested"
        blurb={`Your ${meta.token} settles at ${hourUtc(done.readyAt)}. It keeps earning until then, and you claim it from the Requests tab.`}
        timeline={[
          { label: "Signed in wallet", time: clockText(done.signedAt), state: "done" },
          { label: "Confirmed on Solana", time: clockText(done.confirmedAt), state: "done" },
          { label: "Settles", time: hourUtc(done.readyAt), state: "now" },
        ]}
        rows={[
          { k: "Requested", v: `${fmt(done.amount, 4)} ${meta.token}` },
          { k: "Shares escrowed", v: `${fmt(done.amount, 4)} cv${t}` },
        ]}
        signature={done.signature}
        onDone={close}
      />
    );
  }

  if (!connected || p.shares <= 0)
    return (
      <>
        <p className="tnote">{connected ? `You don't have ${meta.token} in this vault yet.` : "Connect a wallet to see your position."}</p>
        <button className="go ghost" onClick={() => (connected ? setTab("deposit") : onConnect())}>
          {connected ? `Deposit ${meta.token}` : "Connect wallet"}
        </button>
      </>
    );

  // The input is in stock units; one share = one deposited stock unit in the rpc position mapping.
  const sharesRaw = BigInt(sharesRawStr);
  const raw = parseToRaw(amt, decimals);
  const a = rawToNumber(raw, decimals);
  const frac = p.stockAmount > 0 ? Math.min(1, a / p.stockAmount) : 0;
  const preview = redemptionPreview(Math.min(a, p.stockAmount), p.usdcEarned * frac, v.priceUsd, 0);
  const tooMuch = raw > sharesRaw;
  const settleAt = nextTopOfHour(now);

  async function go() {
    if (raw <= 0n) return setErr("Enter how much to withdraw.");
    if (tooMuch) return setErr(`You have ${fmt(p.stockAmount, 4)} ${meta.token} in this vault.`);
    setErr("");
    setBusy(true);
    const signedAt = Date.now();
    try {
      const { nonce, signature } = await requestExit(t, { raw, ui: a }, signer);
      const readyAt = readyAtFor(Date.now());
      addExit(t, { nonce, shares: a, stockAmount: a, usdcAmount: 0, epochId: 0, status: "open", requestedAt: Date.now(), readyAt });
      setDone({ amount: a, signedAt, confirmedAt: Date.now(), signature, readyAt });
      showToast(`Withdrawal of ${fmt(a, 4)} ${meta.token} requested.`);
      setAmt("");
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
      <AmountBox
        value={amt}
        onChange={setAmt}
        unit={meta.token}
        availableLabel={stale ? "Position updating…" : `Position ${fmt(p.stockAmount, 4)} ${meta.token}`}
        availableRaw={sharesRaw}
        decimals={decimals}
        usdText={raw > 0n ? `≈ ${usd(a * v.priceUsd, 2)}` : "≈ $0.00"}
        chips={[25, 50, 75, 100]}
        disabled={busy}
      />
      <div className="err2" role="alert">
        {err || (tooMuch ? `You have ${fmt(p.stockAmount, 4)} ${meta.token} in this vault.` : "")}
      </div>
      <div className="rcpt">
        <div>
          <span>You get back</span>
          <b className="mono">{raw > 0n ? `${fmt(preview.stockOut, 4)} ${meta.token}` : "—"}</b>
        </div>
        <div>
          <span>Plus earned</span>
          <b className={`mono${preview.usdcOut > 0 ? " pos" : ""}`}>{raw > 0n ? `${fmt(preview.usdcOut, 2)} USDC` : "—"}</b>
        </div>
        <div>
          <span>Settles at</span>
          <b className="mono">
            {hourUtc(settleAt)} · in {formatCountdown(settleAt - now).replace(/^0/, "")}
          </b>
        </div>
        <div>
          <span>Exit fee</span>
          <b className="mono">0.00 (headroom available)</b>
        </div>
      </div>
      {preview.stockReduced && a > 0 && <p className="tnote">No USDC yet: a sliver of stock covers the vault&apos;s set-up cost until funding accrues.</p>}
      <button className="go ghost" onClick={go} disabled={busy}>
        {busy ? "Confirming…" : "Request withdrawal"}
      </button>
    </>
  );
}
