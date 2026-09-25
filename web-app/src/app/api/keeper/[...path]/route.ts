// Server-side proxy for the keeper's status server so the browser never needs a route to it.
// Upstream from KEEPER_URL (server-only env, e.g. http://keeper:8787 inside compose);
// the browser uses NEXT_PUBLIC_KEEPER_URL=/api/keeper. GET only: /status, /history, /healthz.
export const runtime = "nodejs";

const ALLOWED = new Set(["status", "history", "healthz"]);

export async function GET(req: Request, ctx: { params: Promise<{ path: string[] }> }) {
  const upstream = process.env.KEEPER_URL;
  if (!upstream) return new Response(JSON.stringify({ error: "KEEPER_URL not set" }), { status: 500 });
  const { path } = await ctx.params;
  if (path.length !== 1 || !ALLOWED.has(path[0])) return new Response("not found", { status: 404 });
  const search = new URL(req.url).search;
  try {
    const r = await fetch(`${upstream.replace(/\/$/, "")}/${path[0]}${search}`, { cache: "no-store" });
    return new Response(await r.text(), {
      status: r.status,
      headers: { "content-type": r.headers.get("content-type") ?? "application/json" },
    });
  } catch {
    // Keeper down or unreachable: the ops page treats non-200 as "keeper unreachable".
    return new Response(JSON.stringify({ error: "keeper unreachable" }), { status: 502, headers: { "content-type": "application/json" } });
  }
}
