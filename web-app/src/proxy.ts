import { NextResponse, type NextRequest } from "next/server";

/**
 * Host-based routing: requests to the app host are served from /app, everything
 * else is the landing site. Set NEXT_PUBLIC_APP_HOST (default app.carrera.xyz).
 * On localhost both surfaces are reachable by path: / and /app.
 */
const APP_HOST = process.env.NEXT_PUBLIC_APP_HOST ?? "app.carrera.xyz";

export function proxy(request: NextRequest) {
  const host = request.headers.get("host") ?? "";
  const { pathname } = request.nextUrl;
  if (host.startsWith(APP_HOST) && !pathname.startsWith("/app")) {
    const url = request.nextUrl.clone();
    url.pathname = pathname === "/" ? "/app" : `/app${pathname}`;
    return NextResponse.rewrite(url);
  }
  return NextResponse.next();
}

export const config = {
  matcher: ["/((?!_next/static|_next/image|favicon.ico|.*\\.(?:svg|png|jpg|ico|css|js)).*)"],
};
