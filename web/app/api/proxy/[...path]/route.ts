import { NextResponse, type NextRequest } from "next/server";
import { clearSessionCookies, readSession, setSessionCookies } from "@/lib/server/cookies";
import { apiBase, describe, problem, problemResponse, refreshTokens } from "@/lib/server/upstream";
import type { TokenResponse } from "@/lib/types";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

type Context = { params: Promise<{ path: string[] }> };

/** Request headers worth forwarding to the API. Everything else stays at the edge. */
const FORWARD_REQUEST_HEADERS = [
  "accept",
  "accept-language",
  "content-type",
  "if-none-match",
  "if-modified-since",
  "x-request-id",
];

/** Response headers passed back to the browser. Encoding and length are recomputed. */
const FORWARD_RESPONSE_HEADERS = [
  "content-type",
  "content-disposition",
  "cache-control",
  "etag",
  "last-modified",
  "location",
  "retry-after",
  "x-request-id",
  "x-ratelimit-limit",
  "x-ratelimit-remaining",
];

/**
 * Catch-all BFF proxy. `/api/proxy/<path>?q` becomes
 * `${API_INTERNAL_URL}/api/v1/<path>?q` with the access token from the cookie.
 * A 401 from the API triggers exactly one refresh (rotating the refresh cookie)
 * and one retry; RFC 7807 bodies and every other response pass through unchanged.
 */
async function handle(req: NextRequest, ctx: Context): Promise<NextResponse> {
  const { path } = await ctx.params;
  const target = `${apiBase()}/${path.map(encodeURIComponent).join("/")}${req.nextUrl.search}`;

  const { access, refresh } = readSession(req);
  if (!access && !refresh) {
    const res = problemResponse(problem(401, "unauthorized", "Unauthorized", "No session"));
    clearSessionCookies(res);
    return res;
  }

  const method = req.method.toUpperCase();
  const body = method === "GET" || method === "HEAD" ? undefined : await req.arrayBuffer();
  const headers = new Headers();
  for (const name of FORWARD_REQUEST_HEADERS) {
    const value = req.headers.get(name);
    if (value) headers.set(name, value);
  }
  if (!headers.has("accept")) headers.set("accept", "application/json, application/problem+json");
  const ip = req.headers.get("x-forwarded-for");
  if (ip) headers.set("x-forwarded-for", ip);

  let token = access;
  let rotated: TokenResponse | null = null;

  try {
    if (!token && refresh) {
      const r = await refreshTokens(refresh);
      if (!r.ok) return sessionLost(r.status, r.problem);
      rotated = r.tokens;
      token = r.tokens.access_token;
    }

    let upstream = await send(target, method, headers, body, token);

    if (upstream.status === 401 && refresh && !rotated) {
      const r = await refreshTokens(refresh);
      if (!r.ok) return sessionLost(r.status, r.problem);
      rotated = r.tokens;
      token = r.tokens.access_token;
      upstream = await send(target, method, headers, body, token);
    }

    const res = passThrough(upstream);
    if (rotated) setSessionCookies(res, rotated);
    if (upstream.status === 401) clearSessionCookies(res);
    return res;
  } catch (err) {
    return problemResponse(problem(502, "upstream_unavailable", "API unavailable", describe(err)));
  }
}

async function send(
  target: string,
  method: string,
  headers: Headers,
  body: ArrayBuffer | undefined,
  token: string | null,
): Promise<Response> {
  const h = new Headers(headers);
  if (token) h.set("authorization", `Bearer ${token}`);
  return fetch(target, {
    method,
    headers: h,
    body: body && body.byteLength > 0 ? body : undefined,
    cache: "no-store",
    redirect: "manual",
  });
}

function passThrough(upstream: Response): NextResponse {
  const headers = new Headers();
  for (const name of FORWARD_RESPONSE_HEADERS) {
    const value = upstream.headers.get(name);
    if (value) headers.set(name, value);
  }
  if (!headers.has("cache-control")) headers.set("cache-control", "no-store");
  const empty = upstream.status === 204 || upstream.status === 304 || upstream.status === 205;
  return new NextResponse(empty ? null : upstream.body, { status: upstream.status, headers });
}

function sessionLost(status: number, p: Parameters<typeof problemResponse>[0]): NextResponse {
  const finalStatus = status >= 500 ? status : 401;
  const res = problemResponse({ ...p, status: finalStatus });
  if (finalStatus === 401) clearSessionCookies(res);
  return res;
}

export const GET = handle;
export const POST = handle;
export const PATCH = handle;
export const PUT = handle;
export const DELETE = handle;
