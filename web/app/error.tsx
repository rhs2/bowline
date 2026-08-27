"use client";

import { ProblemBoundary } from "@/components/ProblemBoundary";

export default function RootError({ error, reset }: { error: Error & { digest?: string }; reset: () => void }) {
  return (
    <main className="min-h-screen px-4">
      <ProblemBoundary error={error} reset={reset} />
    </main>
  );
}
