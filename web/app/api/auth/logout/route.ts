import { NextResponse, type NextRequest } from "next/server";
import { clearSessionCookies, readSession } from "@/lib/server/cookies";
import { upstreamJson } from "@/lib/server/upstream";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

/**
 * POST /api/auth/logout: revokes the refresh token upstream (best effort) and clears
 * every session cookie. Always succeeds from the browser's point of view.
 */
export async function POST(req: NextRequest): Promise<NextResponse> {
  const { access, refresh } = readSession(req);
  if (refresh) {
    try {
      await upstreamJson("/auth/logout", {
        method: "POST",
        body: { refresh_token: refresh },
        accessToken: access,
      });
    } catch {
      // The cookies are cleared regardless; a stale refresh token expires on its own.
    }
  }
  const res = new NextResponse(null, { status: 204, headers: { "cache-control": "no-store" } });
  clearSessionCookies(res);
  return res;
}
