"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, ApiError, buildQuery, isApiError, type Query } from "./api";
import type { ListEnvelope } from "./types";

export function toApiError(err: unknown): ApiError {
  if (isApiError(err)) return err;
  return new ApiError({
    type: "about:blank",
    title: "Request failed",
    status: 0,
    code: "network",
    detail: err instanceof Error ? err.message : String(err),
  });
}

function isAbort(err: unknown): boolean {
  return err instanceof DOMException && err.name === "AbortError";
}

export interface QueryState<T> {
  data: T | null;
  error: ApiError | null;
  loading: boolean;
  reload: () => void;
  /** Replace the cached value locally (after a mutation) without refetching. */
  mutate: (updater: (current: T | null) => T | null) => void;
}

/**
 * Fetch one resource through the proxy. Pass `null` as the path to hold the query
 * (for example until an id is known). Re-fetches whenever path or query changes.
 */
export function useQuery<T>(path: string | null, options?: { query?: Query }): QueryState<T> {
  const key = path === null ? null : `${path}${buildQuery(options?.query)}`;
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const [loading, setLoading] = useState<boolean>(key !== null);
  const [tick, setTick] = useState(0);

  useEffect(() => {
    if (key === null) {
      setData(null);
      setError(null);
      setLoading(false);
      return;
    }
    const controller = new AbortController();
    let active = true;
    setLoading(true);
    api
      .get<T>(key, { signal: controller.signal })
      .then((result) => {
        if (!active) return;
        setData(result);
        setError(null);
        setLoading(false);
      })
      .catch((err: unknown) => {
        if (!active || isAbort(err)) return;
        setError(toApiError(err));
        setLoading(false);
      });
    return () => {
      active = false;
      controller.abort();
    };
  }, [key, tick]);

  const reload = useCallback(() => setTick((t) => t + 1), []);
  const mutate = useCallback((updater: (current: T | null) => T | null) => {
    setData((current) => updater(current));
  }, []);

  return { data, error, loading, reload, mutate };
}

export interface ListState<T> {
  items: T[];
  total: number;
  page: number;
  perPage: number;
  setPage: (page: number) => void;
  loading: boolean;
  error: ApiError | null;
  reload: () => void;
}

/**
 * Paginated list bound to the `{items, page, per_page, total}` envelope. Changing the
 * filters resets to page 1.
 */
export function useList<T>(
  path: string | null,
  filters: Query = {},
  options?: { perPage?: number },
): ListState<T> {
  const perPage = options?.perPage ?? 25;
  const [page, setPage] = useState(1);
  const filterKey = useMemo(() => buildQuery(filters), [filters]);
  const lastFilterKey = useRef(filterKey);

  useEffect(() => {
    if (lastFilterKey.current !== filterKey) {
      lastFilterKey.current = filterKey;
      setPage(1);
    }
  }, [filterKey]);

  const query = useMemo<Query>(() => ({ ...filters, page, per_page: perPage }), [filters, page, perPage]);
  const state = useQuery<ListEnvelope<T>>(path, { query });

  return {
    items: state.data?.items ?? [],
    total: state.data?.total ?? 0,
    page: state.data?.page ?? page,
    perPage: state.data?.per_page ?? perPage,
    setPage,
    loading: state.loading,
    error: state.error,
    reload: state.reload,
  };
}

/** Debounce a fast-changing value (search boxes). */
export function useDebounced<T>(value: T, delayMs = 250): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const id = setTimeout(() => setDebounced(value), delayMs);
    return () => clearTimeout(id);
  }, [value, delayMs]);
  return debounced;
}

/** A ticking clock for countdowns; re-renders every `intervalMs`. */
export function useNow(intervalMs = 30000): Date {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), intervalMs);
    return () => clearInterval(id);
  }, [intervalMs]);
  return now;
}
