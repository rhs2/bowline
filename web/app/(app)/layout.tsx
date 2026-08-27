import type { ReactNode } from "react";
import { AppShell } from "@/components/shell/AppShell";
import { MeProvider } from "@/lib/me";
import { getMe } from "@/lib/server/me";

// Every page under the shell depends on the session cookie; nothing is prerendered.
export const dynamic = "force-dynamic";

export default async function AppLayout({ children }: { children: ReactNode }) {
  const me = await getMe();
  return (
    <MeProvider initial={me}>
      <AppShell>{children}</AppShell>
    </MeProvider>
  );
}
