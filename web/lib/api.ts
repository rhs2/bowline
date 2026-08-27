/**
 * Browser-side API client. Every call goes to the same-origin BFF proxy at
 * `/api/proxy/<path>`, which attaches the access token from the httpOnly cookie and
 * forwards to `${API_INTERNAL_URL}/api/v1/<path>`. Tokens never reach client code.
 *
 * The proxy already refreshes an expired access token once. If a 401 still comes
 * back (for example the process restarted and the in-flight refresh was lost) the
 * client asks `/api/auth/refresh` to rotate the session and retries the call once,
 * sharing a single in-flight refresh between concurrent requests. When that fails
 * too, the session is cleared and the browser goes to /login.
 */

import type { FieldError, ListEnvelope, Problem } from "./types";

/** Accepts either the list envelope or a bare array and returns the rows. */
export function asItems<T>(data: ListEnvelope<T> | T[] | null | undefined): T[] {
  if (!data) return [];
  return Array.isArray(data) ? data : (data.items ?? []);
}

export type QueryValue = string | number | boolean | null | undefined;
export type Query = Record<string, QueryValue>;

export interface RequestOptions {
  query?: Query;
  signal?: AbortSignal;
  headers?: Record<string, string>;
}

export class ApiError extends Error {
  readonly status: number;
  readonly problem: Problem;

  constructor(problem: Problem) {
    super(problem.detail || problem.title || `Request failed (${problem.status})`);
    this.name = "ApiError";
    this.status = problem.status;
    this.problem = problem;
  }

  get code(): string {
    return this.problem.code;
  }

  /** `errors[]` from a validation problem keyed by field name. */
  fieldErrors(): Record<string, string> {
    return fieldErrorMap(this.problem.errors);
  }
}

export function fieldErrorMap(errors: FieldError[] | undefined): Record<string, string> {
  const out: Record<string, string> = {};
  for (const e of errors ?? []) {
    if (!(e.field in out)) out[e.field] = e.message;
  }
  return out;
}

export function isApiError(value: unknown): value is ApiError {
  return value instanceof ApiError;
}

/** A readable message for any thrown value. */
export function errorMessage(value: unknown): string {
  if (isApiError(value)) return value.message;
  if (value instanceof Error) return value.message;
  return "Something went wrong";
}

export function buildQuery(query?: Query): string {
  if (!query) return "";
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) {
    if (value === undefined || value === null || value === "") continue;
    params.set(key, String(value));
  }
  const s = params.toString();
  return s ? `?${s}` : "";
}

const PROXY_BASE = "/api/proxy";
const REFRESH_URL = "/api/auth/refresh";
const LOGOUT_URL = "/api/auth/logout";

interface ApiConfig {
  fetch: typeof fetch;
  onUnauthorized: () => void;
}

const config: ApiConfig = {
  fetch: (...args) => globalThis.fetch(...args),
  onUnauthorized: defaultOnUnauthorized,
};

/** Override fetch or the unauthorized handler (used by tests). */
export function configureApi(overrides: Partial<ApiConfig>): void {
  Object.assign(config, overrides);
}

function defaultOnUnauthorized(): void {
  if (typeof window === "undefined") return;
  if (window.location.pathname === "/login") return;
  const next = encodeURIComponent(window.location.pathname + window.location.search);
  void config
    .fetch(LOGOUT_URL, { method: "POST", credentials: "same-origin" })
    .catch(() => undefined)
    .finally(() => {
      window.location.assign(`/login?next=${next}`);
    });
}

let refreshInFlight: Promise<boolean> | null = null;

/** Rotate the session through the BFF. Concurrent callers share one request. */
export function refreshSession(): Promise<boolean> {
  if (!refreshInFlight) {
    refreshInFlight = config
      .fetch(REFRESH_URL, { method: "POST", credentials: "same-origin" })
      .then((res) => res.ok)
      .catch(() => false)
      .finally(() => {
        refreshInFlight = null;
      });
  }
  return refreshInFlight;
}

function statusCode(status: number): string {
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

export async function problemFromResponse(res: Response): Promise<Problem> {
  const fallback: Problem = {
    type: "about:blank",
    title: res.statusText || `HTTP ${res.status}`,
    status: res.status,
    code: statusCode(res.status),
  };
  try {
    const text = await res.text();
    if (!text) return fallback;
    const parsed: unknown = JSON.parse(text);
    if (parsed && typeof parsed === "object" && "status" in parsed && "title" in parsed) {
      const p = parsed as Partial<Problem>;
      return {
        type: p.type ?? "about:blank",
        title: p.title ?? fallback.title,
        status: typeof p.status === "number" ? p.status : res.status,
        detail: p.detail,
        code: p.code ?? fallback.code,
        request_id: p.request_id,
        errors: Array.isArray(p.errors) ? p.errors : undefined,
      };
    }
    return { ...fallback, detail: typeof parsed === "string" ? parsed : undefined };
  } catch {
    return fallback;
  }
}

async function parseBody<T>(res: Response): Promise<T> {
  if (res.status === 204 || res.status === 205) return undefined as T;
  const text = await res.text();
  if (!text) return undefined as T;
  return JSON.parse(text) as T;
}

async function request<T>(
  method: string,
  path: string,
  body: unknown,
  options: RequestOptions | undefined,
  retried: boolean,
): Promise<T> {
  const url = `${PROXY_BASE}/${path.replace(/^\/+/, "")}${buildQuery(options?.query)}`;
  const headers: Record<string, string> = { accept: "application/json", ...(options?.headers ?? {}) };
  let payload: BodyInit | undefined;
  if (body !== undefined) {
    headers["content-type"] = "application/json";
    payload = JSON.stringify(body);
  }
  const res = await config.fetch(url, {
    method,
    headers,
    body: payload,
    credentials: "same-origin",
    signal: options?.signal,
    cache: "no-store",
  });

  if (res.status === 401) {
    if (!retried && (await refreshSession())) {
      return request<T>(method, path, body, options, true);
    }
    const problem = await problemFromResponse(res);
    config.onUnauthorized();
    throw new ApiError(problem);
  }
  if (!res.ok) {
    throw new ApiError(await problemFromResponse(res));
  }
  return parseBody<T>(res);
}

export const api = {
  get<T>(path: string, options?: RequestOptions): Promise<T> {
    return request<T>("GET", path, undefined, options, false);
  },
  post<T>(path: string, body?: unknown, options?: RequestOptions): Promise<T> {
    return request<T>("POST", path, body ?? {}, options, false);
  },
  patch<T>(path: string, body: unknown, options?: RequestOptions): Promise<T> {
    return request<T>("PATCH", path, body, options, false);
  },
  put<T>(path: string, body: unknown, options?: RequestOptions): Promise<T> {
    return request<T>("PUT", path, body, options, false);
  },
  del<T>(path: string, options?: RequestOptions): Promise<T> {
    return request<T>("DELETE", path, undefined, options, false);
  },
};

/** Absolute proxy URL for links that the browser should open directly (file downloads). */
export function proxyUrl(path: string, query?: Query): string {
  return `${PROXY_BASE}/${path.replace(/^\/+/, "")}${buildQuery(query)}`;
}
