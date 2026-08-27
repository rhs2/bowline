"use client";

import { Suspense, useState, type FormEvent } from "react";
import { useSearchParams } from "next/navigation";
import { AuthCard } from "@/components/AuthCard";
import { Button } from "@/components/ui/Button";
import { FormError, Input } from "@/components/ui/Field";
import { ApiError, fieldErrorMap, problemFromResponse } from "@/lib/api";

function safeNext(value: string | null): string {
  if (!value || !value.startsWith("/") || value.startsWith("//")) return "/dashboard";
  return value;
}

function LoginForm() {
  const params = useSearchParams();
  const next = safeNext(params.get("next"));
  const notice = params.get("notice");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<ApiError | null>(null);

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setPending(true);
    setError(null);
    try {
      const res = await fetch("/api/auth/login", {
        method: "POST",
        headers: { "content-type": "application/json" },
        credentials: "same-origin",
        body: JSON.stringify({ email, password }),
      });
      if (!res.ok) {
        throw new ApiError(await problemFromResponse(res));
      }
      const data = (await res.json()) as { must_change_password: boolean };
      window.location.assign(data.must_change_password ? "/change-password" : next);
    } catch (err) {
      setError(
        err instanceof ApiError
          ? err
          : new ApiError({
              type: "about:blank",
              title: "Sign in failed",
              status: 0,
              code: "network",
              detail: "Could not reach the server. Check your connection and try again.",
            }),
      );
      setPending(false);
    }
  }

  const fields = error ? fieldErrorMap(error.problem.errors) : {};
  const topMessage =
    error && !error.problem.errors?.length
      ? error.status === 401
        ? "Email or password is incorrect."
        : error.status === 423
          ? "This account is locked after too many failed attempts. Try again in a few minutes."
          : error.status === 429
            ? "Too many attempts. Please wait a moment."
            : error.message
      : null;

  return (
    <form onSubmit={onSubmit} className="space-y-4" noValidate>
      {notice === "password-changed" ? (
        <p className="rounded-md border border-emerald-200 bg-emerald-50 px-3 py-2 text-sm text-emerald-800">
          Password changed. Sign in again with your new password.
        </p>
      ) : null}
      <FormError message={topMessage} />
      <Input
        label="Email"
        type="email"
        autoComplete="username"
        value={email}
        onChange={(e) => setEmail(e.target.value)}
        error={fields.email}
        required
        autoFocus
      />
      <Input
        label="Password"
        type="password"
        autoComplete="current-password"
        value={password}
        onChange={(e) => setPassword(e.target.value)}
        error={fields.password}
        required
      />
      <Button type="submit" block loading={pending}>
        Sign in
      </Button>
    </form>
  );
}

export default function LoginPage() {
  return (
    <AuthCard title="Sign in" description="Use your company email and password.">
      <Suspense fallback={null}>
        <LoginForm />
      </Suspense>
    </AuthCard>
  );
}
