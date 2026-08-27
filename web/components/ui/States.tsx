import type { ReactNode } from "react";
import type { ApiError } from "@/lib/api";
import { Button } from "./Button";

export function EmptyState({
  title,
  description,
  action,
}: {
  title: string;
  description?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="flex flex-col items-center justify-center gap-2 py-6 text-center">
      <p className="text-sm font-medium text-slate-700">{title}</p>
      {description ? <p className="max-w-md text-sm text-slate-500">{description}</p> : null}
      {action ? <div className="mt-2">{action}</div> : null}
    </div>
  );
}

/** Renders an RFC 7807 problem the way an operator would want to read it. */
export function ErrorState({ error, onRetry }: { error: ApiError; onRetry?: () => void }) {
  const p = error.problem;
  return (
    <div role="alert" className="rounded-lg border border-red-200 bg-red-50 p-4 text-sm text-red-900">
      <p className="font-semibold">
        {p.title}
        {p.status ? <span className="ml-2 font-normal text-red-700">HTTP {p.status}</span> : null}
      </p>
      {p.detail ? <p className="mt-1">{p.detail}</p> : null}
      {p.errors && p.errors.length > 0 ? (
        <ul className="mt-2 list-inside list-disc">
          {p.errors.map((e, i) => (
            <li key={`${e.field}-${i}`}>
              <span className="font-medium">{e.field}</span>: {e.message}
            </li>
          ))}
        </ul>
      ) : null}
      <p className="mt-2 text-xs text-red-700">
        code {p.code}
        {p.request_id ? <span>, request {p.request_id}</span> : null}
      </p>
      {onRetry ? (
        <div className="mt-3">
          <Button variant="secondary" size="sm" onClick={onRetry}>
            Try again
          </Button>
        </div>
      ) : null}
    </div>
  );
}
