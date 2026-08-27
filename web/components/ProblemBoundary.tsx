"use client";

import { isApiError } from "@/lib/api";
import { Button } from "./ui/Button";
import { ErrorState } from "./ui/States";

/**
 * Shared body for the Next.js `error.tsx` boundaries. API problems render as
 * RFC 7807 details; anything else shows a plain message with the digest.
 */
export function ProblemBoundary({
  error,
  reset,
}: {
  error: Error & { digest?: string };
  reset: () => void;
}) {
  return (
    <div className="mx-auto max-w-2xl py-10">
      {isApiError(error) ? (
        <ErrorState error={error} onRetry={reset} />
      ) : (
        <div role="alert" className="rounded-lg border border-red-200 bg-red-50 p-5 text-sm text-red-900">
          <p className="font-semibold">Something went wrong</p>
          <p className="mt-1">{error.message || "An unexpected error occurred."}</p>
          {error.digest ? <p className="mt-2 text-xs text-red-700">digest {error.digest}</p> : null}
          <div className="mt-4">
            <Button variant="secondary" size="sm" onClick={reset}>
              Try again
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
