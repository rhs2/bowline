"use client";

import { useMemo, useState } from "react";
import Link from "next/link";
import { useMe } from "@/lib/me";
import { useDebounced, useList, useQuery } from "@/lib/hooks";
import { api, asItems } from "@/lib/api";
import { useAction } from "@/lib/forms";
import { formatDateTime, formatRelative, humanize } from "@/lib/format";
import type { AdminUser, ListEnvelope, ResetPasswordResponse, Role } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { DataTable, type Column } from "@/components/ui/DataTable";
import { FilterBar, SearchInput } from "@/components/ui/Filters";
import { Button } from "@/components/ui/Button";
import { Checkbox, FormError, Select } from "@/components/ui/Field";
import { Modal } from "@/components/ui/Modal";
import { StatusBadge } from "@/components/StatusBadge";
import { Badge } from "@/components/ui/Badge";
import { EmptyState } from "@/components/ui/States";
import { OneTimeSecret } from "@/components/OneTimeSecret";

const USER_STATUSES = [
  { value: "active", label: "Active" },
  { value: "locked", label: "Locked" },
  { value: "disabled", label: "Disabled" },
];

export default function AdminUsersPage() {
  const { has, user: me } = useMe();
  const canManageRoles = has("roles:manage");
  const [q, setQ] = useState("");
  const [status, setStatus] = useState("");
  const [editingRoles, setEditingRoles] = useState<AdminUser | null>(null);
  const [resetting, setResetting] = useState<AdminUser | null>(null);
  const debouncedQ = useDebounced(q);

  const filters = useMemo(() => ({ q: debouncedQ, status }), [debouncedQ, status]);
  const list = useList<AdminUser>("admin/users", filters);
  const roles = useQuery<ListEnvelope<Role> | Role[]>(canManageRoles ? "admin/roles" : null);

  const setLock = useAction(
    ({ id, lock }: { id: string; lock: boolean }) => api.post(`admin/users/${id}/${lock ? "lock" : "unlock"}`),
    { successMessage: "Account updated", onSuccess: () => list.reload() },
  );

  const columns: Column<AdminUser>[] = [
    {
      key: "email",
      header: "User",
      render: (u) => (
        <span className="flex flex-col">
          <span className="font-medium text-slate-900">{u.employee_name ?? u.email}</span>
          <span className="text-xs text-slate-500">{u.email}</span>
        </span>
      ),
    },
    {
      key: "roles",
      header: "Roles",
      render: (u) => (
        <span className="flex flex-wrap gap-1">
          {u.roles.length === 0 ? (
            <span className="text-xs text-slate-400">None</span>
          ) : (
            u.roles.map((r) => (
              <Badge key={r} tone="accent">
                {humanize(r)}
              </Badge>
            ))
          )}
        </span>
      ),
    },
    {
      key: "status",
      header: "Status",
      render: (u) => (
        <span className="flex flex-wrap items-center gap-1">
          <StatusBadge status={u.status} />
          {u.must_change_password ? <Badge tone="warning">Must change password</Badge> : null}
          {u.failed_logins > 0 ? <Badge tone="neutral">{u.failed_logins} failed</Badge> : null}
        </span>
      ),
    },
    {
      key: "last_login",
      header: "Last login",
      hideOnMobile: true,
      render: (u) =>
        u.last_login_at ? (
          <span title={formatDateTime(u.last_login_at)}>{formatRelative(u.last_login_at)}</span>
        ) : (
          <span className="text-slate-400">Never</span>
        ),
    },
    {
      key: "locked_until",
      header: "Locked until",
      hideOnMobile: true,
      render: (u) => (u.locked_until ? formatDateTime(u.locked_until) : ""),
    },
    {
      key: "actions",
      header: "",
      align: "right",
      render: (u) => {
        const self = me?.id === u.id;
        return (
          <span className="flex flex-wrap justify-end gap-2">
            {canManageRoles ? (
              <Button variant="secondary" size="sm" onClick={() => setEditingRoles(u)}>
                Roles
              </Button>
            ) : null}
            <Button variant="secondary" size="sm" disabled={self} onClick={() => setResetting(u)}>
              Reset password
            </Button>
            <Button
              variant={u.status === "locked" ? "success" : "secondary"}
              size="sm"
              disabled={self || setLock.pending}
              onClick={() => void setLock.run({ id: u.id, lock: u.status !== "locked" })}
            >
              {u.status === "locked" ? "Unlock" : "Lock"}
            </Button>
          </span>
        );
      },
    },
  ];

  return (
    <div>
      <PageHeader
        title="Users"
        description="Sign-in accounts, their roles and their state. Employee records live under People."
        actions={
          canManageRoles ? (
            <Link href="/admin/roles" className="text-sm font-medium text-accent-700 hover:underline">
              See role definitions
            </Link>
          ) : null
        }
      />

      <FilterBar>
        <SearchInput value={q} onChange={setQ} placeholder="Search email or name" />
        <Select
          aria-label="Status"
          options={USER_STATUSES}
          placeholder="Any status"
          value={status}
          onChange={(e) => setStatus(e.target.value)}
          className="w-full sm:w-40"
        />
      </FilterBar>

      <DataTable
        columns={columns}
        rows={list.items}
        rowKey={(u) => u.id}
        loading={list.loading}
        error={list.error}
        pagination={{ page: list.page, perPage: list.perPage, total: list.total, onPage: list.setPage }}
        empty={<EmptyState title="No users match" description="Try a different search term." />}
      />

      {editingRoles ? (
        <RoleEditor
          user={editingRoles}
          roles={asItems(roles.data)}
          onClose={() => setEditingRoles(null)}
          onSaved={() => {
            setEditingRoles(null);
            list.reload();
          }}
        />
      ) : null}
      {resetting ? (
        <ResetPasswordModal
          user={resetting}
          onClose={() => {
            setResetting(null);
            list.reload();
          }}
        />
      ) : null}
    </div>
  );
}

function RoleEditor({
  user,
  roles,
  onClose,
  onSaved,
}: {
  user: AdminUser;
  roles: Role[];
  onClose: () => void;
  onSaved: () => void;
}) {
  const [selected, setSelected] = useState<string[]>(user.roles);
  const action = useAction(() => api.put(`admin/users/${user.id}/roles`, { roles: selected }), {
    successMessage: "Roles updated",
    onSuccess: onSaved,
  });

  function toggle(key: string, on: boolean) {
    setSelected((current) => (on ? [...new Set([...current, key])] : current.filter((r) => r !== key)));
  }

  const unchanged =
    selected.length === user.roles.length && selected.every((r) => user.roles.includes(r));

  return (
    <Modal
      open
      onClose={onClose}
      title={`Roles for ${user.employee_name ?? user.email}`}
      description="Roles are bundles of permissions. A user may hold several; the widest scope wins."
      size="lg"
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button loading={action.pending} disabled={unchanged} onClick={() => void action.run()}>
            Save roles
          </Button>
        </>
      }
    >
      <div className="space-y-3">
        <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />
        {roles.length === 0 ? (
          <p className="text-sm text-slate-500">Role definitions could not be loaded.</p>
        ) : (
          <ul className="space-y-2">
            {roles.map((role) => (
              <li key={role.key} className="rounded-md border border-slate-200 p-3">
                <Checkbox
                  label={
                    <span className="flex flex-col">
                      <span className="font-medium text-slate-900">{role.name}</span>
                      <span className="text-xs text-slate-500">{role.description}</span>
                    </span>
                  }
                  checked={selected.includes(role.key)}
                  onChange={(e) => toggle(role.key, e.target.checked)}
                />
                <p className="mt-2 text-xs text-slate-500">
                  {role.permissions.length} {role.permissions.length === 1 ? "permission" : "permissions"}
                </p>
              </li>
            ))}
          </ul>
        )}
      </div>
    </Modal>
  );
}

function ResetPasswordModal({ user, onClose }: { user: AdminUser; onClose: () => void }) {
  const [secret, setSecret] = useState<string | null>(null);
  const action = useAction(
    () => api.post<ResetPasswordResponse>(`admin/users/${user.id}/reset-password`),
    {
      successMessage: "Temporary password issued",
      onSuccess: (res) => setSecret(res.temporary_password),
    },
  );

  if (secret) {
    return (
      <Modal
        open
        onClose={onClose}
        title="Temporary password"
        description="This is shown once and cannot be retrieved again. Copy it now and pass it to the user through a channel you trust."
        footer={
          <Button onClick={onClose}>Done</Button>
        }
      >
        <p className="text-sm text-slate-700">
          {user.employee_name ?? user.email} must change this password at their next sign-in.
        </p>
        <OneTimeSecret value={secret} />
      </Modal>
    );
  }

  return (
    <Modal
      open
      onClose={onClose}
      title={`Reset the password for ${user.employee_name ?? user.email}`}
      description="A new temporary password is generated and shown to you once. The user is forced to change it at next sign-in."
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="danger" loading={action.pending} onClick={() => void action.run()}>
            Reset password
          </Button>
        </>
      }
    >
      <FormError message={action.error?.message} />
      <p className="text-sm text-slate-600">
        Their current password stops working immediately and their active sessions are revoked.
      </p>
    </Modal>
  );
}
