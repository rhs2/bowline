import { NextResponse, type NextRequest } from "next/server";
import { clearSessionCookies, setSessionCookies } from "@/lib/server/cookies";
import { describe, problem, problemResponse, readProblem, upstreamJson } from "@/lib/server/upstream";
import type { TokenResponse } from "@/lib/types";

export const runtime = "nodejs";
export const dynamic = "force-dynamic";

interface LoginBody {
  email?: unknown;
  password?: unknown;
}

/**
 * POST /api/auth/login: exchanges credentials with the API and stores the token
 * pair in httpOnly cookies. The browser only ever sees `must_change_password`.
 */
export async function POST(req: NextRequest): Promise<NextResponse> {
  let body: LoginBody;
  try {
    body = (await req.json()) as LoginBody;
  } catch {
    return problemResponse(problem(400, "bad_request", "Bad request", "Body must be JSON"));
  }
  const email = typeof body.email === "string" ? body.email.trim() : "";
  const password = typeof body.password === "string" ? body.password : "";
  const errors = [];
  if (!email) errors.push({ field: "email", message: "Email is required" });
  if (!password) errors.push({ field: "password", message: "Password is required" });
  if (errors.length > 0) {
    return problemResponse({
      ...problem(422, "validation_failed", "Validation failed", "Check the highlighted fields"),
      errors,
    });
  }

  try {
    const upstream = await upstreamJson("/auth/login", { method: "POST", body: { email, password } });
    if (!upstream.ok) {
      const res = problemResponse(await readProblem(upstream));
      clearSessionCookies(res);
      return res;
    }
    const tokens = (await upstream.json()) as TokenResponse;
    const res = NextResponse.json(
      { must_change_password: tokens.must_change_password, expires_in: tokens.expires_in },
      { headers: { "cache-control": "no-store" } },
    );
    setSessionCookies(res, tokens);
    return res;
  } catch (err) {
    return problemResponse(problem(502, "upstream_unavailable", "API unavailable", describe(err)));
  }
}
