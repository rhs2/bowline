"use client";

import { ProblemBoundary } from "@/components/ProblemBoundary";

export default function AppError({ error, reset }: { error: Error & { digest?: string }; reset: () => void }) {
  return <ProblemBoundary error={error} reset={reset} />;
}
