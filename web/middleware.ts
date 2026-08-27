import { NextResponse, type NextRequest } from "next/server";
import { ACCESS_COOKIE, PWCHANGE_COOKIE, REFRESH_COOKIE } from "@/lib/session-names";

const PUBLIC_PATHS = new Set(["/login"]);

/**
 * Edge gate for pages. Route handlers under /api are excluded by the matcher; they
 * do their own token handling. Unauthenticated visitors go to /login (with the
 * requested path in `next`), and accounts flagged for a password change are pinned
 * to /change-password until the flag cookie is cleared by /api/auth/me.
 */
export function middleware(req: NextRequest): NextResponse {
  const { pathname, search } = req.nextUrl;
  if (PUBLIC_PATHS.has(pathname)) return NextResponse.next();

  const hasSession =
    Boolean(req.cookies.get(ACCESS_COOKIE)?.value) || Boolean(req.cookies.get(REFRESH_COOKIE)?.value);
  if (!hasSession) {
    const url = req.nextUrl.clone();
    url.pathname = "/login";
    url.search = "";
    if (pathname !== "/" && pathname !== "/dashboard") {
      url.searchParams.set("next", `${pathname}${search}`);
    }
    return NextResponse.redirect(url);
  }

  const mustChange = req.cookies.get(PWCHANGE_COOKIE)?.value === "1";
  if (mustChange && pathname !== "/change-password") {
    const url = req.nextUrl.clone();
    url.pathname = "/change-password";
    url.search = "";
    return NextResponse.redirect(url);
  }

  return NextResponse.next();
}

export const config = {
  matcher: ["/((?!api/|_next/|favicon.ico|robots.txt|.*\\.(?:svg|png|jpg|jpeg|gif|webp|ico|css|js|map|txt)$).*)"],
};
