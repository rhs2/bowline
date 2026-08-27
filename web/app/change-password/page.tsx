"use client";

import { useState, type FormEvent } from "react";
import Link from "next/link";
import { AuthCard } from "@/components/AuthCard";
import { Button } from "@/components/ui/Button";
import { FormError, Input } from "@/components/ui/Field";
import { api, ApiError, refreshSession } from "@/lib/api";

const MIN_LENGTH = 12;

export default function ChangePasswordPage() {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<ApiError | null>(null);
  const [localErrors, setLocalErrors] = useState<Record<string, string>>({});

  function validate(): boolean {
    const errs: Record<string, string> = {};
    if (!current) errs.current_password = "Enter your current password";
    if (next.length < MIN_LENGTH) errs.new_password = `Use at least ${MIN_LENGTH} characters`;
    else if (next === current) errs.new_password = "Choose a password you have not used just now";
    if (confirm !== next) errs.confirm = "Passwords do not match";
    setLocalErrors(errs);
    return Object.keys(errs).length === 0;
  }

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (!validate()) return;
    setPending(true);
    setError(null);
    try {
      await api.post<void>("auth/change-password", { current_password: current, new_password: next });
      // The API bumps token_version, so the current access token is dead. Rotate the
      // session; if the refresh token was revoked too, sign in again.
      const refreshed = await refreshSession();
      if (!refreshed) {
        await fetch("/api/auth/logout", { method: "POST", credentials: "same-origin" });
        window.location.assign("/login?notice=password-changed");
        return;
      }
      await fetch("/api/auth/me", { credentials: "same-origin", cache: "no-store" });
      window.location.assign("/dashboard");
    } catch (err) {
      setError(err instanceof ApiError ? err : null);
      setPending(false);
    }
  }

  const fields = { ...(error?.fieldErrors() ?? {}), ...localErrors };
  const topMessage =
    error && !error.problem.errors?.length
      ? error.status === 401 || error.status === 422
        ? "The current password is not correct."
        : error.message
      : null;

  return (
    <AuthCard
      title="Change your password"
      description={`Passwords need at least ${MIN_LENGTH} characters. You will stay signed in.`}
    >
      <form onSubmit={onSubmit} className="space-y-4" noValidate>
        <FormError message={topMessage} />
        <Input
          label="Current password"
          type="password"
          autoComplete="current-password"
          value={current}
          onChange={(e) => setCurrent(e.target.value)}
          error={fields.current_password}
          required
          autoFocus
        />
        <Input
          label="New password"
          type="password"
          autoComplete="new-password"
          value={next}
          onChange={(e) => setNext(e.target.value)}
          error={fields.new_password}
          required
        />
        <Input
          label="Confirm new password"
          type="password"
          autoComplete="new-password"
          value={confirm}
          onChange={(e) => setConfirm(e.target.value)}
          error={fields.confirm}
          required
        />
        <Button type="submit" block loading={pending}>
          Update password
        </Button>
        <p className="text-center text-xs text-slate-500">
          <Link href="/dashboard" className="hover:underline">
            Back to the dashboard
          </Link>
        </p>
      </form>
    </AuthCard>
  );
}
