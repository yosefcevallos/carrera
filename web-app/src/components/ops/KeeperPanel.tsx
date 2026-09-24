"use client";

import { ageLabel } from "@/lib/ops-math";
import { shortAddr } from "@/lib/format";
import { useOpsStore } from "@/store/ops-provider";

export default function KeeperPanel() {
  const k = useOpsStore((s) => s.keeper);
  const health = useOpsStore((s) => s.health);
  const syncedAt = useOpsStore((s) => s.syncedAt);
  const global = k.alerts.filter((a) => a.vault === null).sort((a, z) => z.ts - a.ts).slice(0, 3);
  const crit = k.alerts.filter((a) => a.level === "crit").length;

  return (
    <section className="ops-panel" aria-label="Keeper" style={{ marginTop: 24 }}>
      <h3>Keeper</h3>
      <dl className="ops-keeper">
        <div>
          <dt>Instance</dt>
          <dd>
            {k.instance_id || "—"}
            <small>{k.instance_id ? (k.is_leader ? "holds the lease" : "standby") : "no status yet"}</small>
          </dd>
        </div>
        <div>
          <dt>Hourly pass</dt>
          <dd className={k.hourly_ok || !k.last_hourly_run_ts ? "" : "red"}>
            {ageLabel(k.last_hourly_run_ts)}
            <small>{k.last_hourly_run_ts ? (k.hourly_ok ? "completed" : "had failures") : "not run"}</small>
          </dd>
        </div>
        <div>
          <dt>Fast pass</dt>
          <dd className={health === "stale" ? "red" : ""}>
            {ageLabel(k.last_fast_run_ts)}
            <small>healthz {health}{syncedAt ? `, synced ${ageLabel(syncedAt / 1000)}` : ""}</small>
          </dd>
        </div>
        <div>
          <dt>Keeper SOL</dt>
          <dd className={`num${k.sol_balance > 0 && k.sol_balance < 0.5 ? " red" : ""}`}>
            {k.sol_balance.toFixed(2)}
            <small>
              {k.cluster || "—"}
              {k.program_id ? ` · ${shortAddr(k.program_id)}` : ""}
            </small>
          </dd>
        </div>
      </dl>
      <p className="dim" style={{ fontSize: 13, marginTop: 12 }}>
        {k.alerts.length} alert{k.alerts.length === 1 ? "" : "s"} in the last window, {crit} critical. Vault alerts sit under each book.
      </p>
      {global.length > 0 && (
        <ul className="ops-alerts">
          {global.map((a, i) => (
            <li key={i} className={`ops-alert ${a.level}`}>
              <b>{a.message}</b>
              <span>{ageLabel(a.ts)}</span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
