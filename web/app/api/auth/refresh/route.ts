import { NextResponse, type NextRequest } from "next/server";
import { clearSessionCookies, readSession, setSessionCookies } from "@/lib/server/cookies";
import { problem, problemResponse, refreshTokens } from "@/lib/server/upstream";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

/**
 * POST /api/auth/refresh: rotates the refresh token and reissues both cookies.
 * A failed refresh clears the session so the middleware sends the user to /login.
 */
export async function POST(req: NextRequest): Promise<NextResponse> {
  const { refresh } = readSession(req);
  if (!refresh) {
    const res = problemResponse(problem(401, "unauthorized", "Unauthorized", "No session"));
    clearSessionCookies(res);
    return res;
  }
  const result = await refreshTokens(refresh);
  if (!result.ok) {
    const status = result.status >= 500 ? result.status : 401;
    const res = problemResponse({ ...result.problem, status });
    if (status === 401) clearSessionCookies(res);
    return res;
  }
  const res = NextResponse.json(
    {
      expires_in: result.tokens.expires_in,
      must_change_password: result.tokens.must_change_password,
    },
    { headers: { "cache-control": "no-store" } },
  );
  setSessionCookies(res, result.tokens);
  return res;
}
