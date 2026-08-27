"use client";

import { useState, type ReactNode } from "react";
import { api, errorMessage } from "@/lib/api";
import { useToast } from "./ui/Toast";
import { Button, type ButtonSize, type ButtonVariant } from "./ui/Button";
import type { DownloadResponse } from "@/lib/types";

/**
 * Two-step download: ask the API for a short-lived presigned URL, then send the
 * browser to it. The tab is opened synchronously on the click so the popup blocker
 * treats it as user-initiated, and only its location is set once the URL arrives.
 */
export function DownloadButton({
  path,
  children,
  variant = "secondary",
  size = "sm",
  className,
}: {
  path: string;
  children: ReactNode;
  variant?: ButtonVariant;
  size?: ButtonSize;
  className?: string;
}) {
  const toast = useToast();
  const [pending, setPending] = useState(false);

  async function go() {
    const tab = typeof window === "undefined" ? null : window.open("", "_blank", "noopener,noreferrer");
    setPending(true);
    try {
      const res = await api.get<DownloadResponse>(path);
      if (!res?.url) throw new Error("The API did not return a download link");
      if (tab) tab.location.replace(res.url);
      else window.location.assign(res.url);
    } catch (err) {
      tab?.close();
      toast.error(errorMessage(err));
    } finally {
      setPending(false);
    }
  }

  return (
    <Button variant={variant} size={size} className={className} loading={pending} onClick={() => void go()}>
      {children}
    </Button>
  );
}
