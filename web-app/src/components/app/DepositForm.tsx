"use client";

import { useState } from "react";
import { VAULT_META, type Ticker } from "@/constants/vaults";
import { deposit } from "@/lib/chain/actions";
import { parseToRaw, rawToNumber } from "@/lib/amount";
import { hourUtc, nextTopOfHour } from "@/lib/app2";
import { fmt, usd } from "@/lib/format";
import { useRefresh } from "@/lib/use-refresh";
import { useSigner } from "@/lib/use-signer";
import { currentApyBps } from "@/lib/yield";
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
}

export default function DepositForm({ t, onConnect }: { t: Ticker; onConnect: () => void }) {
  const v = useVaultStore((s) => s.vaults[t]);
  const balRawStr = usePositionStore((s) => s.balancesRaw[t]);
  const decimals = usePositionStore((s) => s.decimals[t]);
  const status = useWalletStore((s) => s.status);
  const close = useUiStore((s) => s.closeVaultWindow);
  const showToast = useUiStore((s) => s.showToast);
  const refresh = useRefresh();
  const signer = useSigner();
  const [amt, setAmt] = useState("");
  const [err, setErr] = useState("");
  const [busy, setBusy] = useState(false);
  const [stale, setStale] = useState(false);
  const [done, setDone] = useState<Done | null>(null);
  const meta = VAULT_META[t];
  const connected = status === "connected";
  // Everything that reaches the chain is exact integer math on base units.
  const balRaw = BigInt(balRawStr);
  const bal = rawToNumber(balRaw, decimals);
  const raw = parseToRaw(amt, decimals);
  const a = rawToNumber(raw, decimals);
  const tooMuch = connected && raw > balRaw;
  const apyBps = currentApyBps(v);
  const yearly = (a * v.priceUsd * apyBps) / 10_000;
  const shareName = `cv${t}`;

  async function go() {
    if (!connected) return onConnect();
    if (raw <= 0n) return setErr("Enter how much to deposit.");
    if (tooMuch) return setErr(`That's more than the ${fmt(bal, 4)} ${meta.token} in your wallet.`);
    setErr("");
    setBusy(true);
    const signedAt = Date.now();
    try {
      const signature = await deposit(t, { raw, ui: a }, signer);
      setDone({ amount: a, signedAt, confirmedAt: Date.now(), signature });
      showToast(`Deposited ${fmt(a, 4)} ${meta.token}.`);
      setAmt("");
    } catch (e) {
      setErr(e instanceof Error ? e.message : "Deposit failed.");
    } finally {
      // Success or failure: pull balances, positions and vaults from RPC right away.
      setStale(true);
      setBusy(false);
      await refresh();
      setStale(false);
    }
  }

  if (done) {
    const starts = hourUtc(nextTopOfHour(done.confirmedAt));
    return (
      <Receipt
        title="Deposit confirmed"
        blurb={`Your ${meta.token} is in the vault and starts earning at the next funding cycle, ${starts}.`}
        timeline={[
          { label: "Signed in wallet", time: clockText(done.signedAt), state: "done" },
          { label: "Confirmed on Solana", time: clockText(done.confirmedAt), state: "done" },
          { label: "Earning starts", time: starts, state: "now" },
        ]}
        rows={[
          { k: "Deposited", v: `${fmt(done.amount, 4)} ${meta.token}` },
          { k: "Shares minted", v: `${fmt(done.amount, 4)} ${shareName}` },
        ]}
        signature={done.signature}
        onDone={close}
      />
    );
  }

  return (
    <>
      <AmountBox
        value={amt}
        onChange={setAmt}
        unit={meta.token}
        availableLabel={!connected ? "Connect a wallet" : stale ? "Balance updating…" : `Balance ${fmt(bal, 4)} ${meta.token}`}
        availableRaw={connected ? balRaw : 0n}
        decimals={decimals}
        usdText={raw > 0n ? `≈ ${usd(a * v.priceUsd, 2)}` : "≈ $0.00"}
        chips={[10, 25, 50, 100]}
        disabled={busy}
      />
      <div className="err2" role="alert">
        {err || (tooMuch ? `That's more than the ${fmt(bal, 4)} ${meta.token} in your wallet.` : "")}
      </div>
      {v.mode !== "funding" && (
        <p className="tnote">
          {v.mode === "parked"
            ? `Funding on ${meta.name} is low, so the loan is supplied on Kamino for now. It switches back on its own.`
            : `Waiting for funding: the loan is repaid until ${meta.name}'s funding clears the hurdle. Deposits are accepted now.`}
        </p>
      )}
      <div className="rcpt">
        <div>
          <span>You receive</span>
          <b className="mono">{raw > 0n ? `${fmt(a, 4)} ${shareName}` : "—"}</b>
        </div>
        <div>
          <span>Price exposure kept</span>
          <b className="mono">100%</b>
        </div>
        <div>
          <span>Est. yearly</span>
          <b className={`mono${yearly > 0 ? " pos" : ""}`}>{raw > 0n && apyBps > 0 ? `+${fmt(yearly, 2)} USDC` : "0.00 USDC"}</b>
        </div>
        <div>
          <span>Performance fee</span>
          <b className="mono">15% of yield</b>
        </div>
        <div>
          <span>Settles</span>
          <b className="mono">1 transaction</b>
        </div>
      </div>
      <button className="go" onClick={go} disabled={busy}>
        {connected ? (busy ? "Confirming…" : `Deposit ${meta.token}`) : "Connect wallet to deposit"}
      </button>
    </>
  );
}
