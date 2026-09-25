// Server-side JSON-RPC proxy so the browser never sees the upstream RPC key.
// Upstream comes from SERVER_RPC_URL (server-only env); the browser uses NEXT_PUBLIC_RPC_URL=/api/rpc.
export const runtime = "nodejs";

export async function POST(req: Request) {
  const upstream = process.env.SERVER_RPC_URL;
  if (!upstream) return new Response(JSON.stringify({ error: "SERVER_RPC_URL not set" }), { status: 500 });
  const body = await req.text();
  const r = await fetch(upstream, { method: "POST", headers: { "content-type": "application/json" }, body, cache: "no-store" });
  return new Response(await r.text(), { status: r.status, headers: { "content-type": "application/json" } });
}
