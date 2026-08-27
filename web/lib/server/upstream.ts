/**
 * Server-only helpers for talking to the Rust API from route handlers. Nothing in
 * here is imported by client components.
 */

import { NextResponse } from "next/server";
import type { Problem, TokenResponse } from "@/lib/types";

export function apiBase(): string {
  const root = (process.env.API_INTERNAL_URL || "http://localhost:8080").replace(/\/+$/, "");
  return `${root}/api/v1`;
}

export const PROBLEM_CONTENT_TYPE = "application/problem+json";

export function problem(status: number, code: string, title: string, detail?: string): Problem {
  return { type: "about:blank", title, status, code, detail };
}

/** An RFC 7807 response with the problem's own status. */
export function problemResponse(p: Problem): NextResponse {
  return NextResponse.json(p, {
    status: p.status,
    headers: { "content-type": PROBLEM_CONTENT_TYPE, "cache-control": "no-store" },
  });
}

/** JSON call to the upstream API. Returns the raw response; callers decide on status. */
export async function upstreamJson(
  path: string,
  init: { method?: string; body?: unknown; accessToken?: string | null; headers?: Record<string, string> } = {},
): Promise<Response> {
  const headers: Record<string, string> = {
    accept: "application/json, application/problem+json",
    ...(init.headers ?? {}),
  };
  if (init.body !== undefined) headers["content-type"] = "application/json";
  if (init.accessToken) headers.authorization = `Bearer ${init.accessToken}`;
  return fetch(`${apiBase()}${path}`, {
    method: init.method ?? "GET",
    headers,
    body: init.body === undefined ? undefined : JSON.stringify(init.body),
    cache: "no-store",
    redirect: "manual",
  });
}

export type RefreshResult =
  | { ok: true; tokens: TokenResponse }
  | { ok: false; status: number; problem: Problem };

const inFlight = new Map<string, Promise<RefreshResult>>();

/**
 * Exchange a refresh token for a new pair. Refresh tokens rotate on every use and a
 * replay revokes the whole family, so concurrent requests carrying the same token
 * must share one upstream call; the map below does that per process.
 */
export function refreshTokens(refreshToken: string): Promise<RefreshResult> {
  const existing = inFlight.get(refreshToken);
  if (existing) return existing;
  const p = doRefresh(refreshToken).finally(() => {
    inFlight.delete(refreshToken);
  });
  inFlight.set(refreshToken, p);
  return p;
}

async function doRefresh(refreshToken: string): Promise<RefreshResult> {
  try {
    const res = await upstreamJson("/auth/refresh", {
      method: "POST",
      body: { refresh_token: refreshToken },
    });
    if (res.ok) {
      const tokens = (await res.json()) as TokenResponse;
      return { ok: true, tokens };
    }
    return { ok: false, status: res.status, problem: await readProblem(res) };
  } catch (err) {
    return {
      ok: false,
      status: 502,
      problem: problem(502, "upstream_unavailable", "API unavailable", describe(err)),
    };
  }
}

export async function readProblem(res: Response): Promise<Problem> {
  const fallback = problem(res.status, codeFor(res.status), res.statusText || `HTTP ${res.status}`);
  try {
    const text = await res.text();
    if (!text) return fallback;
    const parsed = JSON.parse(text) as Partial<Problem>;
    if (parsed && typeof parsed === "object" && typeof parsed.status === "number") {
      return { ...fallback, ...parsed } as Problem;
    }
    return fallback;
  } catch {
    return fallback;
  }
}

function codeFor(status: number): string {
  switch (status) {
    case 401:
      return "unauthorized";
    case 403:
      return "forbidden";
    case 404:
      return "not_found";
    case 409:
      return "conflict";
    case 422:
      return "validation_failed";
    case 423:
      return "locked";
    case 429:
      return "rate_limited";
    default:
      return status >= 500 ? "internal" : "error";
  }
}

export function describe(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}
