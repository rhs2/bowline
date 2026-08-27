"use client";

import { useMemo, useState } from "react";
import { useQuery } from "@/lib/hooks";
import { asItems } from "@/lib/api";
import { humanize } from "@/lib/format";
import { parseKey } from "@/lib/permissions";
import type { ListEnvelope, Role } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { Card, CardBody, CardHeader } from "@/components/ui/Card";
import { SearchInput } from "@/components/ui/Filters";
import { Badge } from "@/components/ui/Badge";
import { EmptyState, ErrorState } from "@/components/ui/States";
import { CardSkeleton } from "@/components/ui/Skeleton";

/** Group a role's permission keys by their `resource:action` family. */
function groupPermissions(permissions: string[]): Array<{ family: string; scopes: string[] }> {
  const map = new Map<string, string[]>();
  for (const key of [...permissions].sort()) {
    const { family, scope } = parseKey(key);
    const list = map.get(family) ?? [];
    list.push(scope ?? "");
    map.set(family, list);
  }
  return [...map.entries()].map(([family, scopes]) => ({ family, scopes }));
}

export default function RolesPage() {
  const roles = useQuery<ListEnvelope<Role> | Role[]>("admin/roles");
  const [q, setQ] = useState("");

  const rows = asItems(roles.data);
  const filtered = useMemo(() => {
    const needle = q.trim().toLowerCase();
    if (!needle) return rows;
    return rows.filter(
      (r) =>
        r.name.toLowerCase().includes(needle) ||
        r.key.toLowerCase().includes(needle) ||
        r.description.toLowerCase().includes(needle) ||
        r.permissions.some((p) => p.toLowerCase().includes(needle)),
    );
  }, [rows, q]);

  return (
    <div>
      <PageHeader
        title="Roles"
        description="Each role is a bundle of permission keys. A user may hold several roles, and the widest scope in a family wins."
      />

      <div className="mb-4">
        <SearchInput value={q} onChange={setQ} placeholder="Search a role or a permission key" />
      </div>

      {roles.error ? (
        <ErrorState error={roles.error} onRetry={roles.reload} />
      ) : roles.loading && rows.length === 0 ? (
        <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
          <CardSkeleton lines={5} />
          <CardSkeleton lines={5} />
        </div>
      ) : filtered.length === 0 ? (
        <Card>
          <CardBody>
            <EmptyState title="No roles match" description="Try a different search term." />
          </CardBody>
        </Card>
      ) : (
        <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
          {filtered.map((role) => (
            <Card key={role.key}>
              <CardHeader
                title={
                  <span className="flex flex-wrap items-center gap-2">
                    {role.name}
                    <span className="font-mono text-xs font-normal text-slate-500">{role.key}</span>
                  </span>
                }
                description={role.description}
                actions={<Badge tone="neutral">{role.permissions.length}</Badge>}
              />
              <CardBody>
                {role.permissions.length === 0 ? (
                  <p className="text-sm text-slate-500">This role carries no permissions of its own.</p>
                ) : (
                  <ul className="space-y-1.5">
                    {groupPermissions(role.permissions).map((group) => (
                      <li key={group.family} className="flex flex-wrap items-baseline gap-2">
                        <span className="font-mono text-xs text-slate-700">{group.family}</span>
                        <span className="flex flex-wrap gap-1">
                          {group.scopes.map((scope, i) => (
                            <Badge key={`${group.family}-${scope}-${i}`} tone={scope === "all" ? "warning" : "accent"}>
                              {scope === "" ? "granted" : humanize(scope)}
                            </Badge>
                          ))}
                        </span>
                      </li>
                    ))}
                  </ul>
                )}
              </CardBody>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
