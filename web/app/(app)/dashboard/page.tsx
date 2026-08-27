"use client";

import Link from "next/link";
import { useMe } from "@/lib/me";
import { useQuery } from "@/lib/hooks";
import { fullName, humanize, levelName } from "@/lib/format";
import { canBroadcast, isLeaveApprover, isSupportAgent } from "@/lib/permissions";
import { statCards } from "@/lib/dashboard";
import type { Dashboard } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { Card, CardBody, CardHeader, Stat } from "@/components/ui/Card";
import { CardSkeleton } from "@/components/ui/Skeleton";
import { ErrorState } from "@/components/ui/States";
import { Badge } from "@/components/ui/Badge";

export default function DashboardPage() {
  const { employee, roles, permissions, has, can } = useMe();
  const dash = useQuery<Dashboard>("dashboard");

  const cards = dash.data ? statCards(dash.data) : [];

  const quickLinks: Array<{ href: string; label: string; show: boolean }> = [
    { href: "/hr/attendance", label: "Clock in or out", show: has("attendance:record:self") },
    { href: "/ops/work-orders", label: "My work orders", show: has("tasks:read:self") },
    { href: "/hr/leave?tab=request", label: "Request leave", show: has("leave:request") },
    { href: "/support?new=1", label: "Open a support ticket", show: has("tickets:create") },
    { href: "/inbox?compose=1", label: "New message", show: true },
    {
      href: "/announcements?compose=1",
      label: "Post an announcement",
      show: canBroadcast(permissions),
    },
    {
      href: "/hr/leave?tab=approvals",
      label: "Review leave requests",
      show: isLeaveApprover(permissions),
    },
    { href: "/support?tab=all", label: "Support queue", show: isSupportAgent(permissions) },
    { href: "/ops/shipments?new=1", label: "New shipment", show: has("shipments:write") },
    { href: "/finance/expenses?tab=new", label: "Submit an expense", show: has("expenses:submit") },
    { href: "/finance/invoices?new=1", label: "Draft an invoice", show: has("invoices:draft") },
    { href: "/finance/ledger?tab=new", label: "Post a journal entry", show: has("ledger:post") },
    { href: "/people", label: "People directory", show: can("employees:read:subtree") },
    { href: "/admin/users", label: "Manage users", show: has("users:manage") },
  ].filter((q) => q.show);

  return (
    <div>
      <PageHeader
        title={employee ? `Hello, ${employee.first_name}` : "Dashboard"}
        description={
          employee
            ? `${employee.title}, ${employee.department_name}. ${levelName(employee.level)} level.`
            : undefined
        }
        meta={
          <span className="flex flex-wrap gap-1">
            {roles.map((r) => (
              <Badge key={r} tone="accent">
                {humanize(r)}
              </Badge>
            ))}
          </span>
        }
      />

      {dash.error ? (
        <div className="mb-6">
          <ErrorState error={dash.error} onRetry={dash.reload} />
        </div>
      ) : null}

      <section
        aria-label="Summary"
        className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3"
      >
        {dash.loading && !dash.data
          ? Array.from({ length: 4 }).map((_, i) => <CardSkeleton key={i} lines={1} />)
          : cards.map((c) => (
              <Stat key={c.key} label={c.label} value={c.value} hint={c.hint} href={c.href} />
            ))}
        {!dash.loading && dash.data && cards.length === 0 ? (
          <Card className="sm:col-span-2 lg:col-span-3">
            <CardBody>
              <p className="text-sm text-slate-600">Nothing needs your attention right now.</p>
            </CardBody>
          </Card>
        ) : null}
      </section>

      <div className="mt-6 grid grid-cols-1 gap-4 lg:grid-cols-3">
        <Card className="lg:col-span-2">
          <CardHeader title="Quick actions" />
          <CardBody>
            <ul className="grid grid-cols-1 gap-2 sm:grid-cols-2">
              {quickLinks.map((q) => (
                <li key={q.href}>
                  <Link
                    href={q.href}
                    className="block rounded-md border border-slate-200 px-3 py-2.5 text-sm font-medium text-slate-800 hover:border-accent-300 hover:bg-accent-50"
                  >
                    {q.label}
                  </Link>
                </li>
              ))}
            </ul>
          </CardBody>
        </Card>
        <ChainCard />
      </div>
    </div>
  );
}

function ChainCard() {
  const { chain, employee } = useMe();
  const sorted = [...chain].sort((a, b) => a.level - b.level);
  return (
    <Card>
      <CardHeader title="Chain of command" description="Who you report up to" />
      <CardBody>
        {sorted.length === 0 ? (
          <p className="text-sm text-slate-500">You are at the top of the tree.</p>
        ) : (
          <ol className="space-y-2">
            {sorted.map((link) => (
              <li key={link.id} className="flex items-center gap-3 text-sm">
                <span className="w-6 shrink-0 text-xs font-semibold text-slate-400">
                  L{link.level}
                </span>
                <Link href={`/people/${link.id}`} className="min-w-0 flex-1 hover:text-accent-700">
                  <span className="block truncate font-medium text-slate-900">
                    {link.name}
                    {employee && link.id === employee.id ? " (you)" : ""}
                  </span>
                  <span className="block truncate text-xs text-slate-500">{link.title}</span>
                </Link>
              </li>
            ))}
          </ol>
        )}
        {employee ? (
          <p className="mt-3 text-xs text-slate-500">
            Signed in as {fullName(employee)}, {employee.employee_no}.
          </p>
        ) : null}
      </CardBody>
    </Card>
  );
}
