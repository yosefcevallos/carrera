// Per-viewer convenience: exit requests this browser sent, keyed by wallet, so a request shows in
// the Requests tab before the indexer has seen it. localStorage can be absent or throw (private
// windows, blocked site data), so every access is guarded and the app works without it.

export interface LocalExit {
  nonce: string;
  /** Unix ms */
  requestedAt: number;
  /** Shares in base units, decimal string */
  sharesRaw: string;
}

const KEY = (address: string) => `carrera:exits:${address}`;

export function readLocalExits(address: string): Record<string, LocalExit[]> {
  try {
    const raw = localStorage.getItem(KEY(address));
    return raw ? (JSON.parse(raw) as Record<string, LocalExit[]>) : {};
  } catch {
    return {};
  }
}

export function rememberLocalExit(address: string, ticker: string, exit: LocalExit): void {
  try {
    const all = readLocalExits(address);
    const list = (all[ticker] ?? []).filter((e) => e.nonce !== exit.nonce);
    all[ticker] = [exit, ...list].slice(0, 50);
    localStorage.setItem(KEY(address), JSON.stringify(all));
  } catch {
    /* per-viewer convenience only */
  }
}

/** Drop nonces the chain and the indexer both no longer know about. */
export function forgetLocalExits(address: string, ticker: string, nonces: string[]): void {
  if (!nonces.length) return;
  try {
    const all = readLocalExits(address);
    all[ticker] = (all[ticker] ?? []).filter((e) => !nonces.includes(e.nonce));
    localStorage.setItem(KEY(address), JSON.stringify(all));
  } catch {
    /* ignore */
  }
}

const FINAL_KEY = (address: string) => `carrera:exits-final:${address}`;

/** Final statuses this browser produced (redeem / cancel confirmed), so a lagging indexer never shows them as settled again. */
export function readFinalExits(address: string): Record<string, Record<string, "redeemed" | "cancelled">> {
  try {
    const raw = localStorage.getItem(FINAL_KEY(address));
    return raw ? (JSON.parse(raw) as Record<string, Record<string, "redeemed" | "cancelled">>) : {};
  } catch {
    return {};
  }
}

export function rememberFinalExit(address: string, ticker: string, nonce: string, status: "redeemed" | "cancelled"): void {
  try {
    const all = readFinalExits(address);
    all[ticker] = { ...(all[ticker] ?? {}), [nonce]: status };
    localStorage.setItem(FINAL_KEY(address), JSON.stringify(all));
  } catch {
    /* per-viewer convenience only */
  }
}
