import { NextResponse, type NextRequest } from "next/server";
import {
  clearSessionCookies,
  readSession,
  setPasswordChangeFlag,
  setSessionCookies,
} from "@/lib/server/cookies";
import {
  describe,
  problem,
  problemResponse,
  readProblem,
  refreshTokens,
  upstreamJson,
} from "@/lib/server/upstream";
import type { Me, TokenResponse } from "@/lib/types";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

/**
 * GET /api/auth/me: the current principal. Refreshes once when the access token has
 * expired, and keeps the password-change flag cookie in sync with the API's view so
 * the middleware redirect lifts as soon as the password is changed.
 */
export async function GET(req: NextRequest): Promise<NextResponse> {
  const { access, refresh } = readSession(req);
  if (!access && !refresh) {
    const res = problemResponse(problem(401, "unauthorized", "Unauthorized", "No session"));
    clearSessionCookies(res);
    return res;
  }

  let rotated: TokenResponse | null = null;
  let token = access;

  try {
    if (!token && refresh) {
      const r = await refreshTokens(refresh);
      if (!r.ok) return failed(r.status, r.problem);
      rotated = r.tokens;
      token = r.tokens.access_token;
    }

    let upstream = await upstreamJson("/auth/me", { accessToken: token });
    if (upstream.status === 401 && refresh && !rotated) {
      const r = await refreshTokens(refresh);
      if (!r.ok) return failed(r.status, r.problem);
      rotated = r.tokens;
      token = r.tokens.access_token;
      upstream = await upstreamJson("/auth/me", { accessToken: token });
    }

    if (!upstream.ok) {
      const p = await readProblem(upstream);
      if (upstream.status === 401) return failed(401, p);
      return problemResponse(p);
    }

    const me = (await upstream.json()) as Me;
    const res = NextResponse.json(me, { headers: { "cache-control": "no-store" } });
    if (rotated) setSessionCookies(res, rotated);
    setPasswordChangeFlag(res, Boolean(me.user?.must_change_password));
    return res;
  } catch (err) {
    return problemResponse(problem(502, "upstream_unavailable", "API unavailable", describe(err)));
  }
}

function failed(status: number, p: Parameters<typeof problemResponse>[0]): NextResponse {
  const finalStatus = status >= 500 ? status : 401;
  const res = problemResponse({ ...p, status: finalStatus });
  if (finalStatus === 401) clearSessionCookies(res);
  return res;
}
