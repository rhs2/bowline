"use client";

import { useEffect, useState, type ReactNode } from "react";
import { usePathname } from "next/navigation";
import { useMe } from "@/lib/me";
import { Sidebar } from "./Sidebar";
import { TopBar } from "./TopBar";
import { PageSkeleton } from "@/components/ui/Skeleton";
import { ErrorState } from "@/components/ui/States";

export function AppShell({ children }: { children: ReactNode }) {
  const { me, loading, error, reload } = useMe();
  const [menuOpen, setMenuOpen] = useState(false);
  const pathname = usePathname();

  useEffect(() => {
    setMenuOpen(false);
  }, [pathname]);

  return (
    <div className="min-h-screen bg-slate-50">
      <Sidebar open={menuOpen} onClose={() => setMenuOpen(false)} permissions={me?.permissions ?? []} />
      <div className="flex min-h-screen flex-col lg:pl-64">
        <TopBar onMenu={() => setMenuOpen(true)} />
        <main className="mx-auto w-full max-w-7xl flex-1 px-4 py-6 sm:px-6 lg:px-8">
          {me ? (
            children
          ) : loading ? (
            <PageSkeleton />
          ) : error ? (
            <ErrorState error={error} onRetry={() => void reload()} />
          ) : (
            <PageSkeleton />
          )}
        </main>
      </div>
    </div>
  );
}
