"use client";

import { useCallback, useRef, useState } from "react";
import type { ApiError } from "./api";
import { toApiError } from "./hooks";
import { useToast } from "@/components/ui/Toast";

export interface ActionOptions<R> {
  successMessage?: string;
  onSuccess?: (result: R) => void;
  /** Show a toast for failures other than validation problems (default true). */
  toastErrors?: boolean;
}

export interface ActionState<Args extends unknown[], R> {
  run: (...args: Args) => Promise<R | undefined>;
  pending: boolean;
  error: ApiError | null;
  /** Field-level messages from a 422 `errors[]` array, keyed by field. */
  fieldErrors: Record<string, string>;
  reset: () => void;
}

/**
 * Wraps a mutation with pending state, problem capture and toasts. Validation
 * problems (422) are kept for inline display; everything else is toasted.
 */
export function useAction<Args extends unknown[], R>(
  fn: (...args: Args) => Promise<R>,
  options: ActionOptions<R> = {},
): ActionState<Args, R> {
  const toast = useToast();
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<ApiError | null>(null);
  const fnRef = useRef(fn);
  fnRef.current = fn;
  const optionsRef = useRef(options);
  optionsRef.current = options;

  const run = useCallback(
    async (...args: Args): Promise<R | undefined> => {
      setPending(true);
      setError(null);
      try {
        const result = await fnRef.current(...args);
        const opts = optionsRef.current;
        if (opts.successMessage) toast.success(opts.successMessage);
        opts.onSuccess?.(result);
        return result;
      } catch (err) {
        const apiErr = toApiError(err);
        setError(apiErr);
        if (optionsRef.current.toastErrors !== false && apiErr.status !== 422) {
          toast.error(apiErr.message);
        }
        return undefined;
      } finally {
        setPending(false);
      }
    },
    [toast],
  );

  const reset = useCallback(() => setError(null), []);

  return { run, pending, error, fieldErrors: error?.fieldErrors() ?? {}, reset };
}
