import { cookies } from "next/headers";
import type { Me } from "@/lib/types";
import { ACCESS_COOKIE } from "./cookies";
import { upstreamJson } from "./upstream";

/**
 * Server-component helper: the current principal, or null when there is no usable
 * access token. This never refreshes (a server component cannot set cookies); the
 * client `MeProvider` falls back to `/api/auth/me`, which can rotate the session.
 */
export async function getMe(): Promise<Me | null> {
  const store = await cookies();
  const access = store.get(ACCESS_COOKIE)?.value;
  if (!access) return null;
  try {
    const res = await upstreamJson("/auth/me", { accessToken: access });
    if (!res.ok) return null;
    return (await res.json()) as Me;
  } catch {
    return null;
  }
}
