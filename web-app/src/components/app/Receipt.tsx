"use client";

import { shortSig } from "@/lib/app2";

export interface TimelineStep {
  label: string;
  /** Right-hand mono time text */
  time: string;
  state: "done" | "now" | "todo";
}

export interface ReceiptRow {
  k: string;
  v: string;
}

const clock = (ms: number) => new Date(ms).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false });
export const clockText = clock;

/**
 * Transaction receipt shown in the ticket body once an action confirms: a green check, one line
 * of explanation, a dashed timeline, dashed rows, the signature (linked to Solscan) and Done.
 */
export default function Receipt({ title, blurb, timeline, rows, signature, onDone, doneLabel = "Done" }: {
  title: string;
  blurb: string;
  timeline: TimelineStep[];
  rows: ReceiptRow[];
  signature: string;
  onDone: () => void;
  doneLabel?: string;
}) {
  return (
    <div className="rcpt-wrap" role="status">
      <div className="ok">
        <i aria-hidden="true">✓</i>
        <b>{title}</b>
      </div>
      <p className="blurb">{blurb}</p>
      <div className="tl">
        {timeline.map((s) => (
          <div key={s.label} className={s.state === "done" ? "d" : s.state === "now" ? "n" : ""}>
            <i />
            <span>{s.label}</span>
            <em className="mono">{s.time}</em>
          </div>
        ))}
      </div>
      <div className="rcpt">
        {rows.map((r) => (
          <div key={r.k}>
            <span>{r.k}</span>
            <b className="mono">{r.v}</b>
          </div>
        ))}
        <div>
          <span>Signature</span>
          {signature ? (
            <a className="mono sig" href={`https://solscan.io/tx/${signature}`} target="_blank" rel="noreferrer">
              {shortSig(signature)} ↗
            </a>
          ) : (
            <b className="mono">demo, not sent</b>
          )}
        </div>
      </div>
      <button className="go ghost" onClick={onDone}>
        {doneLabel}
      </button>
    </div>
  );
}
