"use client";

import { Suspense, useState, type FormEvent } from "react";
import { useSearchParams } from "next/navigation";
import { useMe } from "@/lib/me";
import { useList, useQuery } from "@/lib/hooks";
import { api, asItems } from "@/lib/api";
import { useAction } from "@/lib/forms";
import { formatDateTime, humanize } from "@/lib/format";
import { canBroadcast } from "@/lib/permissions";
import type { AnnouncementScope, Department, ListEnvelope, Thread, ThreadDetail } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { Button } from "@/components/ui/Button";
import { Input, Select, Textarea, FormError } from "@/components/ui/Field";
import { Modal } from "@/components/ui/Modal";
import { Badge } from "@/components/ui/Badge";
import { EmptyState, ErrorState } from "@/components/ui/States";
import { Skeleton, PageSkeleton } from "@/components/ui/Skeleton";
import { Pagination } from "@/components/ui/DataTable";

export default function AnnouncementsPage() {
  return (
    <Suspense fallback={<PageSkeleton />}>
      <Announcements />
    </Suspense>
  );
}

function Announcements() {
  const params = useSearchParams();
  const { permissions } = useMe();
  const [composing, setComposing] = useState(params.get("compose") === "1");
  const [open, setOpen] = useState<string | null>(null);
  const list = useList<Thread>("comms/threads", { kind: "announcement" });

  return (
    <div>
      <PageHeader
        title="Announcements"
        description="Company, department and team-wide notices"
        actions={canBroadcast(permissions) ? <Button onClick={() => setComposing(true)}>New announcement</Button> : null}
      />
      {list.error ? (
        <ErrorState error={list.error} onRetry={list.reload} />
      ) : list.loading && list.items.length === 0 ? (
        <div className="space-y-3">
          {Array.from({ length: 4 }).map((_, i) => (
            <Skeleton key={i} className="h-20" />
          ))}
        </div>
      ) : list.items.length === 0 ? (
        <div className="rounded-lg border border-slate-200 bg-white p-8 shadow-card">
          <EmptyState title="No announcements yet" />
        </div>
      ) : (
        <div className="overflow-hidden rounded-lg border border-slate-200 bg-white shadow-card">
          <ul className="divide-y divide-slate-100">
            {list.items.map((t) => (
              <li key={t.id}>
                <button
                  type="button"
                  onClick={() => setOpen(open === t.id ? null : t.id)}
                  className="flex w-full items-start justify-between gap-3 px-4 py-3 text-left hover:bg-slate-50"
                  aria-expanded={open === t.id}
                >
                  <div className="min-w-0">
                    <p className={t.unread_count > 0 ? "font-semibold text-slate-900" : "font-medium text-slate-800"}>
                      {t.subject}
                    </p>
                    <p className="mt-0.5 text-xs text-slate-500">
                      {t.last_message?.sender_name ?? t.created_by_name ?? "Bowline"}, {formatDateTime(t.created_at)}
                    </p>
                  </div>
                  <div className="flex shrink-0 items-center gap-2">
                    {t.audience ? <Badge tone="info">{humanize(t.audience.scope)}</Badge> : null}
                    {t.unread_count > 0 ? <Badge tone="accent">New</Badge> : null}
                  </div>
                </button>
                {open === t.id ? <AnnouncementBody id={t.id} onRead={list.reload} /> : null}
              </li>
            ))}
          </ul>
          <Pagination page={list.page} perPage={list.perPage} total={list.total} onPage={list.setPage} />
        </div>
      )}
      {composing ? (
        <ComposeAnnouncement
          onClose={() => setComposing(false)}
          onSent={() => {
            setComposing(false);
            list.reload();
          }}
        />
      ) : null}
    </div>
  );
}

function AnnouncementBody({ id, onRead }: { id: string; onRead: () => void }) {
  const detail = useQuery<ThreadDetail>(`comms/threads/${id}`);
  const [notified, setNotified] = useState(false);
  if (detail.data && !notified) {
    setNotified(true);
    onRead();
  }
  if (detail.error) {
    return (
      <div className="px-4 pb-4">
        <ErrorState error={detail.error} onRetry={detail.reload} />
      </div>
    );
  }
  if (!detail.data) return <Skeleton className="mx-4 mb-4 h-16" />;
  return (
    <div className="space-y-3 border-t border-slate-100 bg-slate-50 px-4 py-4">
      {detail.data.messages.map((m) => (
        <div key={m.id}>
          <p className="text-xs text-slate-500">
            {m.sender_name ?? "Unknown"}, {formatDateTime(m.sent_at)}
          </p>
          <p className="mt-1 whitespace-pre-wrap text-sm text-slate-900">{m.body}</p>
        </div>
      ))}
    </div>
  );
}

function ComposeAnnouncement({ onClose, onSent }: { onClose: () => void; onSent: () => void }) {
  const { has, employee } = useMe();
  const company = has("messages:broadcast:company");
  const subtree = has("messages:broadcast:subtree") || company;
  const scopes: Array<{ value: AnnouncementScope; label: string }> = [
    ...(company ? [{ value: "company" as const, label: "Whole company" }] : []),
    ...(company ? [{ value: "department" as const, label: "A department" }] : []),
    ...(subtree ? [{ value: "subtree" as const, label: "Everyone who reports up to me" }] : []),
  ];
  const [scope, setScope] = useState<AnnouncementScope>(scopes[0]?.value ?? "subtree");
  const [departmentId, setDepartmentId] = useState("");
  const [subject, setSubject] = useState("");
  const [body, setBody] = useState("");
  const departments = useQuery<ListEnvelope<Department> | Department[]>(scope === "department" ? "org/departments" : null);

  const action = useAction(
    () =>
      api.post("comms/announcements", {
        scope,
        ref: scope === "department" ? departmentId : scope === "subtree" ? (employee?.id ?? undefined) : undefined,
        subject,
        body,
      }),
    { successMessage: "Announcement sent", onSuccess: onSent },
  );
  const fe = action.fieldErrors;
  const ready = subject.trim() && body.trim() && (scope !== "department" || departmentId);

  return (
    <Modal
      open
      onClose={onClose}
      title="New announcement"
      description="Fans out to everyone in the audience at send time and emails each recipient."
      size="lg"
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>Cancel</Button>
          <Button type="submit" form="announce" loading={action.pending} disabled={!ready}>Send</Button>
        </>
      }
    >
      <form
        id="announce"
        className="space-y-3"
        onSubmit={(e: FormEvent) => {
          e.preventDefault();
          void action.run();
        }}
      >
        <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />
        <Select
          label="Audience"
          options={scopes}
          value={scope}
          onChange={(e) => setScope(e.target.value as AnnouncementScope)}
          error={fe.scope}
        />
        {scope === "department" ? (
          <Select
            label="Department"
            options={asItems(departments.data).map((d) => ({ value: d.id, label: d.name }))}
            placeholder="Choose a department"
            value={departmentId}
            onChange={(e) => setDepartmentId(e.target.value)}
            error={fe.ref}
            required
          />
        ) : null}
        <Input label="Subject" value={subject} onChange={(e) => setSubject(e.target.value)} error={fe.subject} required />
        <Textarea label="Message" rows={8} value={body} onChange={(e) => setBody(e.target.value)} error={fe.body} required />
      </form>
    </Modal>
  );
}
