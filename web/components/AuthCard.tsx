import type { ReactNode } from "react";

/** Centered card used by the login and password-change screens. */
export function AuthCard({ title, description, children }: { title: string; description?: ReactNode; children: ReactNode }) {
  return (
    <main className="flex min-h-screen items-center justify-center bg-slate-50 px-4 py-10">
      <div className="w-full max-w-sm">
        <div className="mb-6 flex items-center justify-center gap-2">
          <span className="flex h-9 w-9 items-center justify-center rounded-md bg-accent-600 text-base font-bold text-white">
            B
          </span>
          <span className="text-lg font-semibold tracking-tight text-slate-900">Bowline</span>
        </div>
        <div className="rounded-lg border border-slate-200 bg-white p-6 shadow-card">
          <h1 className="text-lg font-semibold text-slate-900">{title}</h1>
          {description ? <p className="mt-1 text-sm text-slate-500">{description}</p> : null}
          <div className="mt-5">{children}</div>
        </div>
      </div>
    </main>
  );
}
