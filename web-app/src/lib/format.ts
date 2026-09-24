export const fmt = (v: number, d = 2) =>
  v.toLocaleString("en-US", { minimumFractionDigits: d, maximumFractionDigits: d });

export const usd = (v: number, d = 0) => (v < 0 ? "−" : "") + "$" + fmt(Math.abs(v), d);

export const usdCompact = (v: number) => {
  if (v >= 1_000_000) return "$" + fmt(v / 1_000_000, 2) + "M";
  if (v >= 1_000) return "$" + fmt(v / 1_000, 0) + "K";
  return usd(v);
};

export const pct = (bps: number, d = 1) => fmt(bps / 100, d) + "%";

export const shortAddr = (a: string) => a.slice(0, 4) + "…" + a.slice(-4);

export const greeting = (hour = new Date().getHours()) => (hour < 17 ? "Buongiorno." : "Buonasera.");
