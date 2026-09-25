"use client";

import { chipRaw } from "@/lib/app2";
import { formatRaw, parseToRaw } from "@/lib/amount";

const clean = (s: string) => s.replace(/[^0-9.]/g, "").replace(/(\..*)\./g, "$1");

/**
 * Order-ticket amount: "Amount" label with the available balance on the right, a mono input in a
 * bordered box with the unit, the ≈ USD line, and percentage chips that fill from the raw balance.
 */
export default function AmountBox({
  value, onChange, unit, availableLabel, availableRaw, decimals, usdText, chips, disabled = false,
}: {
  value: string;
  onChange: (s: string) => void;
  unit: string;
  availableLabel: string;
  availableRaw: bigint;
  decimals: number;
  usdText: string;
  chips: number[];
  disabled?: boolean;
}) {
  const raw = parseToRaw(value, decimals);
  return (
    <>
      <div className="lbl">
        <span>Amount</span>
        <span className="mono">{availableLabel}</span>
      </div>
      <div className="amt">
        <input
          id="amt"
          className="n mono"
          inputMode="decimal"
          autoComplete="off"
          placeholder="0.0000"
          value={value}
          disabled={disabled}
          onChange={(e) => onChange(clean(e.target.value))}
          aria-label={`Amount in ${unit}`}
        />
        <span className="u">{unit}</span>
      </div>
      <div className="lbl usd">
        <span className="mono">{usdText}</span>
        <span />
      </div>
      <div className="pct" role="group" aria-label="Percentage of balance">
        {chips.map((p) => {
          const target = chipRaw(availableRaw, p);
          const on = availableRaw > 0n && raw === target;
          return (
            <button key={p} className={`mono${on ? " on" : ""}`} disabled={disabled || availableRaw === 0n} onClick={() => onChange(formatRaw(target, decimals))}>
              {p >= 100 ? "Max" : `${p}%`}
            </button>
          );
        })}
      </div>
    </>
  );
}
