"use client";

import { Suspense, useMemo, useState, type FormEvent } from "react";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { useMe } from "@/lib/me";
import { useList } from "@/lib/hooks";
import { api } from "@/lib/api";
import { useAction } from "@/lib/forms";
import { formatRelative } from "@/lib/format";
import { isSupportAgent } from "@/lib/permissions";
import { ticketCategoryOptions, ticketPriorityOptions, ticketStatusOptions } from "@/lib/options";
import type { Ticket, TicketCategory, TicketPriority } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { DataTable, type Column } from "@/components/ui/DataTable";
import { FilterBar } from "@/components/ui/Filters";
import { Tabs } from "@/components/ui/Tabs";
import { Button } from "@/components/ui/Button";
import { Select, Input, Textarea, FormError } from "@/components/ui/Field";
import { Modal } from "@/components/ui/Modal";
import { StatusBadge } from "@/components/StatusBadge";
import { SlaCountdown } from "@/components/SlaCountdown";
import { EmptyState } from "@/components/ui/States";
import { PageSkeleton } from "@/components/ui/Skeleton";

type Tab = "mine" | "all";

export default function SupportPage() {
  return (
    <Suspense fallback={<PageSkeleton />}>
      <Support />
    </Suspense>
  );
}

function Support() {
  const router = useRouter();
  const params = useSearchParams();
  const { permissions, has } = useMe();
  const agent = isSupportAgent(permissions);
  const canSeeAll = agent || has("tickets:read:all");
  const [tab, setTab] = useState<Tab>(params.get("tab") === "all" && canSeeAll ? "all" : "mine");
  const [status, setStatus] = useState("");
  const [priority, setPriority] = useState("");
  const [category, setCategory] = useState("");
  const [creating, setCreating] = useState(params.get("new") === "1");

  const filters = useMemo(
    () => ({ mine: tab === "mine" ? 1 : undefined, status, priority, category }),
    [tab, status, priority, category],
  );
  const list = useList<Ticket>("support/tickets", filters);

  const columns: Column<Ticket>[] = [
    { key: "no", header: "Ticket", render: (t) => <span className="font-mono text-xs">{t.ticket_no}</span> },
    {
      key: "subject",
      header: "Subject",
      render: (t) => (
        <Link href={`/support/${t.id}`} className="font-medium text-slate-900 hover:text-accent-700">
          {t.subject}
        </Link>
      ),
    },
    { key: "category", header: "Category", render: (t) => t.category.toUpperCase() === t.category ? t.category : t.category === "it" ? "IT" : t.category === "hr" ? "HR" : t.category, hideOnMobile: true },
    { key: "priority", header: "Priority", render: (t) => <StatusBadge status={t.priority} /> },
    { key: "status", header: "Status", render: (t) => <StatusBadge status={t.status} /> },
    ...(tab === "all"
      ? [{ key: "requester", header: "Requester", render: (t: Ticket) => t.requester_name ?? "", hideOnMobile: true }]
      : []),
    { key: "assignee", header: "Assignee", render: (t) => t.assignee_name ?? <span className="text-slate-400">Unassigned</span>, hideOnMobile: true },
    { key: "sla", header: "SLA", render: (t) => <SlaCountdown ticket={t} /> },
    { key: "created", header: "Opened", render: (t) => formatRelative(t.created_at), hideOnMobile: true },
  ];

  return (
    <div>
      <PageHeader
        title="Support desk"
        description="Requests to the Service Desk, with response-time targets by priority"
        actions={has("tickets:create") ? <Button onClick={() => setCreating(true)}>New ticket</Button> : null}
      />
      {canSeeAll ? (
        <div className="mb-4">
          <Tabs<Tab>
            tabs={[
              { key: "mine", label: "My tickets" },
              { key: "all", label: "All tickets" },
            ]}
            value={tab}
            onChange={setTab}
          />
        </div>
      ) : null}
      <FilterBar>
        <Select aria-label="Status" options={ticketStatusOptions} placeholder="Any status" value={status} onChange={(e) => setStatus(e.target.value)} className="w-full sm:w-48" />
        <Select aria-label="Priority" options={ticketPriorityOptions} placeholder="Any priority" value={priority} onChange={(e) => setPriority(e.target.value)} className="w-full sm:w-40" />
        <Select aria-label="Category" options={ticketCategoryOptions} placeholder="Any category" value={category} onChange={(e) => setCategory(e.target.value)} className="w-full sm:w-40" />
      </FilterBar>
      <DataTable
        columns={columns}
        rows={list.items}
        rowKey={(t) => t.id}
        loading={list.loading}
        error={list.error}
        onRowClick={(t) => router.push(`/support/${t.id}`)}
        pagination={{ page: list.page, perPage: list.perPage, total: list.total, onPage: list.setPage }}
        empty={<EmptyState title="No tickets" description={tab === "mine" ? "Tickets you open appear here." : "The queue is empty."} />}
      />
      {creating ? (
        <NewTicketModal
          onClose={() => setCreating(false)}
          onCreated={(t) => {
            setCreating(false);
            router.push(`/support/${t.id}`);
          }}
        />
      ) : null}
    </div>
  );
}

function NewTicketModal({ onClose, onCreated }: { onClose: () => void; onCreated: (t: Ticket) => void }) {
  const [category, setCategory] = useState<TicketCategory>("it");
  const [priority, setPriority] = useState<TicketPriority>("normal");
  const [subject, setSubject] = useState("");
  const [body, setBody] = useState("");
  const action = useAction(() => api.post<Ticket>("support/tickets", { category, priority, subject, body }), {
    successMessage: "Ticket opened",
    onSuccess: onCreated,
  });
  const fe = action.fieldErrors;
  return (
    <Modal
      open
      onClose={onClose}
      title="New support ticket"
      description="Urgent: 1 hour to first response. High: 4 hours. Normal: 24 hours. Low: 72 hours."
      size="lg"
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>Cancel</Button>
          <Button type="submit" form="new-ticket" loading={action.pending} disabled={!subject.trim() || !body.trim()}>Open ticket</Button>
        </>
      }
    >
      <form
        id="new-ticket"
        className="space-y-3"
        onSubmit={(e: FormEvent) => {
          e.preventDefault();
          void action.run();
        }}
      >
        <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <Select label="Category" options={ticketCategoryOptions} value={category} onChange={(e) => setCategory(e.target.value as TicketCategory)} error={fe.category} />
          <Select label="Priority" options={ticketPriorityOptions} value={priority} onChange={(e) => setPriority(e.target.value as TicketPriority)} error={fe.priority} />
        </div>
        <Input label="Subject" value={subject} onChange={(e) => setSubject(e.target.value)} error={fe.subject} required />
        <Textarea label="Describe the problem" rows={6} value={body} onChange={(e) => setBody(e.target.value)} error={fe.body} required />
      </form>
    </Modal>
  );
}
