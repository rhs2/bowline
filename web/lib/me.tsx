"use client";

import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { ApiError, problemFromResponse } from "./api";
import { can, canAny, has } from "./permissions";
import type { ChainLink, Employee, Me, MeUser } from "./types";

interface MeContextValue {
  me: Me | null;
  loading: boolean;
  error: ApiError | null;
  reload: () => Promise<void>;
}

const MeContext = createContext<MeContextValue | null>(null);

/**
 * Holds the current principal for the app shell. `initial` comes from the server
 * layout when a valid access token was present; otherwise the provider fetches
 * `/api/auth/me`, which can rotate the session through the BFF.
 */
export function MeProvider({ initial, children }: { initial: Me | null; children: ReactNode }) {
  const [me, setMe] = useState<Me | null>(initial);
  const [loading, setLoading] = useState<boolean>(initial === null);
  const [error, setError] = useState<ApiError | null>(null);

  const reload = useCallback(async () => {
    setLoading(true);
    try {
      const res = await fetch("/api/auth/me", { credentials: "same-origin", cache: "no-store" });
      if (res.status === 401) {
        if (typeof window !== "undefined") window.location.assign("/login");
        return;
      }
      if (!res.ok) throw new ApiError(await problemFromResponse(res));
      setMe((await res.json()) as Me);
      setError(null);
    } catch (err) {
      setError(err instanceof ApiError ? err : new ApiError(unknownProblem(err)));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (initial === null) void reload();
  }, [initial, reload]);

  const value = useMemo<MeContextValue>(() => ({ me, loading, error, reload }), [me, loading, error, reload]);
  return <MeContext.Provider value={value}>{children}</MeContext.Provider>;
}

function unknownProblem(err: unknown) {
  return {
    type: "about:blank",
    title: "Could not load your profile",
    status: 0,
    code: "network",
    detail: err instanceof Error ? err.message : undefined,
  };
}

export interface UseMe extends MeContextValue {
  user: MeUser | null;
  employee: Employee | null;
  roles: string[];
  permissions: string[];
  chain: ChainLink[];
  /** Exact key or a wider scope in the same family. */
  can: (key: string) => boolean;
  /** Any scope suffix for the family. */
  canAny: (family: string) => boolean;
  /** Exact key. */
  has: (key: string) => boolean;
  hasRole: (role: string) => boolean;
}

export function useMe(): UseMe {
  const ctx = useContext(MeContext);
  if (!ctx) throw new Error("useMe must be used inside MeProvider");
  const permissions = useMemo(() => ctx.me?.permissions ?? [], [ctx.me]);
  const roles = useMemo(() => ctx.me?.roles ?? [], [ctx.me]);
  return {
    ...ctx,
    user: ctx.me?.user ?? null,
    employee: ctx.me?.employee ?? null,
    roles,
    permissions,
    chain: ctx.me?.chain ?? [],
    can: (key) => can(permissions, key),
    canAny: (family) => canAny(permissions, family),
    has: (key) => has(permissions, key),
    hasRole: (role) => roles.includes(role),
  };
}
