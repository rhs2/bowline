/**
 * Session cookies written by the BFF route handlers. All three are httpOnly and
 * sameSite=lax; `secure` is set in production. The password-change flag is not a
 * secret, it only lets the edge middleware redirect without calling the API.
 */

import type { NextRequest, NextResponse } from "next/server";
import type { TokenResponse } from "@/lib/types";

export const ACCESS_COOKIE = "bowline_access";
export const REFRESH_COOKIE = "bowline_refresh";
export const PWCHANGE_COOKIE = "bowline_pwchange";

const DEFAULT_REFRESH_TTL_SECONDS = 30 * 24 * 60 * 60;

export function refreshTtlSeconds(): number {
  const raw = process.env.REFRESH_TOKEN_TTL_SECONDS;
  const n = raw ? Number(raw) : NaN;
  return Number.isFinite(n) && n > 0 ? n : DEFAULT_REFRESH_TTL_SECONDS;
}

function baseOptions() {
  return {
    httpOnly: true,
    sameSite: "lax" as const,
    secure: process.env.NODE_ENV === "production",
    path: "/",
  };
}

export function setSessionCookies(res: NextResponse, tokens: TokenResponse): void {
  const base = baseOptions();
  res.cookies.set(ACCESS_COOKIE, tokens.access_token, {
    ...base,
    maxAge: Math.max(60, tokens.expires_in),
  });
  res.cookies.set(REFRESH_COOKIE, tokens.refresh_token, { ...base, maxAge: refreshTtlSeconds() });
  setPasswordChangeFlag(res, tokens.must_change_password);
}

export function setPasswordChangeFlag(res: NextResponse, mustChange: boolean): void {
  const base = baseOptions();
  if (mustChange) {
    res.cookies.set(PWCHANGE_COOKIE, "1", { ...base, maxAge: refreshTtlSeconds() });
  } else {
    res.cookies.set(PWCHANGE_COOKIE, "", { ...base, maxAge: 0 });
  }
}

export function clearSessionCookies(res: NextResponse): void {
  const base = baseOptions();
  for (const name of [ACCESS_COOKIE, REFRESH_COOKIE, PWCHANGE_COOKIE]) {
    res.cookies.set(name, "", { ...base, maxAge: 0 });
  }
}

export function readSession(req: NextRequest): { access: string | null; refresh: string | null } {
  return {
    access: req.cookies.get(ACCESS_COOKIE)?.value ?? null,
    refresh: req.cookies.get(REFRESH_COOKIE)?.value ?? null,
  };
}
