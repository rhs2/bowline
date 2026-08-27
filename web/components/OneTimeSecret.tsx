"use client";

import { useState } from "react";
import { Button } from "./ui/Button";

/**
 * A secret the API returns exactly once (a temporary password). It is never fetched
 * again, so the copy affordance sits next to it and the caller is told plainly that
 * closing the dialog loses it.
 */
export function OneTimeSecret({ value }: { value: string }) {
  const [copied, setCopied] = useState(false);
  async function copy() {
    try {
      await navigator.clipboard.writeText(value);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      setCopied(false);
    }
  }
  return (
    <div className="mt-3 flex items-center gap-2 rounded-md border border-amber-200 bg-amber-50 p-3">
      <code className="flex-1 select-all break-all font-mono text-sm text-slate-900">{value}</code>
      <Button variant="secondary" size="sm" onClick={() => void copy()}>
        {copied ? "Copied" : "Copy"}
      </Button>
    </div>
  );
}
