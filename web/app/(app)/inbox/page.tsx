"use client";

import { Suspense, useCallback, useEffect, useMemo, useState, type FormEvent } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import clsx from "clsx";
import { useMe } from "@/lib/me";
import { useList, useQuery } from "@/lib/hooks";
import { api } from "@/lib/api";
import { useAction } from "@/lib/forms";
import { formatDateTime, formatRelative, humanize } from "@/lib/format";
import { importanceOptions } from "@/lib/options";
import type { Importance, Recipient, Thread, ThreadDetail, ThreadKind } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { Button } from "@/components/ui/Button";
import { Input, Select, Textarea, FormError } from "@/components/ui/Field";
import { Modal } from "@/components/ui/Modal";
import { Badge } from "@/components/ui/Badge";
import { EmptyState, ErrorState } from "@/components/ui/States";
import { Skeleton } from "@/components/ui/Skeleton";
import { Avatar } from "@/components/ui/Avatar";
import { RecipientPicker } from "@/components/pickers/RecipientPicker";
import { PageSkeleton } from "@/components/ui/Skeleton";

type Filter = "all" | "unread" | "direct" | "ticket" | "announcement";

const FILTERS: Array<{ key: Filter; label: string }> = [
  { key: "all", label: "All" },
  { key: "unread", label: "Unread" },
  { key: "direct", label: "Direct" },
  { key: "ticket", label: "Tickets" },
  { key: "announcement", label: "Announcements" },
];

export default function InboxPage() {
  return (
    <Suspense fallback={<PageSkeleton />}>
      <Inbox />
    </Suspense>
  );
}

function Inbox() {
  const router = useRouter();
  const params = useSearchParams();
  const selected = params.get("thread");
  const [filter, setFilter] = useState<Filter>("all");
  const [composing, setComposing] = useState(params.get("compose") === "1");

  const filters = useMemo(() => {
    const f: Record<string, string | number> = {};
    if (filter === "unread") f.unread = 1;
    else if (filter !== "all") f.kind = filter;
    return f;
  }, [filter]);
  const list = useList<Thread>("comms/threads", filters, { perPage: 50 });

  const select = useCallback(
    (id: string | null) => {
      const next = new URLSearchParams(params.toString());
      if (id) next.set("thread", id);
      else next.delete("thread");
      next.delete("compose");
      router.replace(`/inbox${next.toString() ? `?${next.toString()}` : ""}`);
    },
    [params, router],
  );

  return (
    <div className="flex h-[calc(100vh-8.5rem)] flex-col">
      <PageHeader title="Inbox" actions={<Button onClick={() => setComposing(true)}>New message</Button>} />
      <div className="flex min-h-0 flex-1 overflow-hidden rounded-lg border border-slate-200 bg-white shadow-card">
        <div className={clsx("flex w-full flex-col border-r border-slate-200 md:w-80 lg:w-96", selected && "hidden md:flex")}>
          <div className="flex gap-1 overflow-x-auto border-b border-slate-200 px-2 py-2">
            {FILTERS.map((f) => (
              <button
                key={f.key}
                type="button"
                onClick={() => setFilter(f.key)}
                className={clsx(
                  "shrink-0 rounded-full px-3 py-1 text-xs font-medium",
                  filter === f.key ? "bg-accent-600 text-white" : "text-slate-600 hover:bg-slate-100",
                )}
              >
                {f.label}
              </button>
            ))}
          </div>
          <ThreadList list={list} selected={selected} onSelect={select} />
        </div>
        <div className={clsx("min-w-0 flex-1 flex-col", selected ? "flex" : "hidden md:flex")}>
          {selected ? (
            <ThreadView id={selected} onBack={() => select(null)} onChanged={list.reload} />
          ) : (
            <div className="flex flex-1 items-center justify-center p-8">
              <EmptyState title="Select a conversation" description="Pick a thread on the left, or start a new message." />
            </div>
          )}
        </div>
      </div>
      {composing ? (
        <ComposeModal
          onClose={() => setComposing(false)}
          onSent={(thread) => {
            setComposing(false);
            list.reload();
            select(thread.id);
          }}
        />
      ) : null}
    </div>
  );
}

function ThreadList({
  list,
  selected,
  onSelect,
}: {
  list: ReturnType<typeof useList<Thread>>;
  selected: string | null;
  onSelect: (id: string) => void;
}) {
  if (list.loading && list.items.length === 0) {
    return (
      <div className="space-y-3 p-3">
        {Array.from({ length: 6 }).map((_, i) => (
          <Skeleton key={i} className="h-12" />
        ))}
      </div>
    );
  }
  if (list.error) {
    return (
      <div className="p-3">
        <ErrorState error={list.error} onRetry={list.reload} />
      </div>
    );
  }
  if (list.items.length === 0) {
    return (
      <div className="p-6">
        <EmptyState title="No conversations" description="Messages you send or receive appear here." />
      </div>
    );
  }
  return (
    <ul className="flex-1 divide-y divide-slate-100 overflow-y-auto">
      {list.items.map((t) => {
        const active = t.id === selected;
        const unread = t.unread_count > 0;
        return (
          <li key={t.id}>
            <button
              type="button"
              onClick={() => onSelect(t.id)}
              className={clsx("block w-full px-3 py-3 text-left hover:bg-slate-50", active && "bg-accent-50")}
            >
              <div className="flex items-center justify-between gap-2">
                <span className={clsx("truncate text-sm", unread ? "font-semibold text-slate-900" : "text-slate-800")}>
                  {t.subject}
                </span>
                <span className="shrink-0 text-xs text-slate-500">{formatRelative(t.last_message_at)}</span>
              </div>
              <div className="mt-0.5 flex items-center gap-2">
                <KindBadge kind={t.kind} />
                <span className="truncate text-xs text-slate-500">
                  {t.last_message?.sender_name ? `${t.last_message.sender_name}: ` : ""}
                  {t.last_message?.body ?? ""}
                </span>
                {unread ? (
                  <span className="ml-auto shrink-0 rounded-full bg-accent-600 px-1.5 text-[11px] font-semibold leading-5 text-white">
                    {t.unread_count}
                  </span>
                ) : null}
              </div>
            </button>
          </li>
        );
      })}
    </ul>
  );
}

function KindBadge({ kind }: { kind: ThreadKind }) {
  if (kind === "direct") return null;
  return <Badge tone={kind === "ticket" ? "warning" : "info"}>{humanize(kind)}</Badge>;
}

function ThreadView({ id, onBack, onChanged }: { id: string; onBack: () => void; onChanged: () => void }) {
  const { employee } = useMe();
  const thread = useQuery<ThreadDetail>(`comms/threads/${id}`);
  const [body, setBody] = useState("");
  const [importance, setImportance] = useState<Importance>("normal");

  useEffect(() => {
    if (thread.data) onChanged();
    // Opening a thread marks it read on the server; refresh the list's unread counts once.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [thread.data?.id]);

  const reply = useAction(
    () => api.post(`comms/threads/${id}/messages`, { body, importance }),
    {
      onSuccess: () => {
        setBody("");
        setImportance("normal");
        thread.reload();
        onChanged();
      },
    },
  );
  const archive = useAction(() => api.post(`comms/threads/${id}/archive`), {
    successMessage: "Thread archived",
    onSuccess: () => {
      onChanged();
      onBack();
    },
  });

  const t = thread.data;

  return (
    <>
      <div className="flex items-start gap-3 border-b border-slate-200 px-4 py-3">
        <button type="button" onClick={onBack} className="rounded p-1 text-slate-500 hover:bg-slate-100 md:hidden" aria-label="Back to list">
          <span aria-hidden="true">&larr;</span>
        </button>
        <div className="min-w-0 flex-1">
          {t ? (
            <>
              <h2 className="truncate text-base font-semibold text-slate-900">{t.subject}</h2>
              <p className="truncate text-xs text-slate-500">
                {t.participants.map((p) => p.name).join(", ")}
              </p>
            </>
          ) : (
            <Skeleton className="h-5 w-48" />
          )}
        </div>
        {t ? (
          <div className="flex items-center gap-2">
            <KindBadge kind={t.kind} />
            <Button variant="ghost" size="sm" onClick={() => void archive.run()} loading={archive.pending}>
              Archive
            </Button>
          </div>
        ) : null}
      </div>
      <div className="flex-1 space-y-4 overflow-y-auto px-4 py-4">
        {thread.error ? (
          <ErrorState error={thread.error} onRetry={thread.reload} />
        ) : !t ? (
          <div className="space-y-3">
            <Skeleton className="h-16" />
            <Skeleton className="h-16 w-2/3" />
          </div>
        ) : (
          t.messages.map((m) => {
            const mine = m.sender_id === employee?.id;
            return (
              <div key={m.id} className={clsx("flex gap-3", mine && "flex-row-reverse")}>
                <Avatar name={m.sender_name ?? "?"} size="sm" />
                <div className={clsx("max-w-[80%] rounded-lg px-3 py-2 text-sm", mine ? "bg-accent-50" : "bg-slate-100")}>
                  <div className="flex items-baseline gap-2 text-xs text-slate-500">
                    <span className="font-medium text-slate-700">{m.sender_name ?? "Unknown"}</span>
                    <span>{formatDateTime(m.sent_at)}</span>
                    {m.importance === "high" ? <Badge tone="warning">High</Badge> : null}
                  </div>
                  <p className="mt-1 whitespace-pre-wrap text-slate-900">{m.body}</p>
                </div>
              </div>
            );
          })
        )}
      </div>
      {t && t.kind !== "announcement" ? (
        <form
          className="border-t border-slate-200 px-4 py-3"
          onSubmit={(e: FormEvent) => {
            e.preventDefault();
            if (body.trim()) void reply.run();
          }}
        >
          <FormError message={reply.error?.message} />
          <Textarea
            aria-label="Reply"
            rows={3}
            value={body}
            onChange={(e) => setBody(e.target.value)}
            placeholder="Write a reply"
            error={reply.fieldErrors.body}
          />
          <div className="mt-2 flex items-center justify-between gap-2">
            <Select
              aria-label="Importance"
              options={importanceOptions}
              value={importance}
              onChange={(e) => setImportance(e.target.value as Importance)}
              className="w-32"
            />
            <Button type="submit" loading={reply.pending} disabled={!body.trim()}>
              Send
            </Button>
          </div>
        </form>
      ) : t ? (
        <p className="border-t border-slate-200 px-4 py-3 text-xs text-slate-500">Announcements do not take replies.</p>
      ) : null}
    </>
  );
}

function ComposeModal({ onClose, onSent }: { onClose: () => void; onSent: (thread: Thread) => void }) {
  const [recipients, setRecipients] = useState<Recipient[]>([]);
  const [subject, setSubject] = useState("");
  const [body, setBody] = useState("");
  const action = useAction(
    () =>
      api.post<Thread>("comms/threads", {
        recipient_ids: recipients.map((r) => r.id),
        subject,
        body,
      }),
    { successMessage: "Message sent", onSuccess: onSent },
  );
  const fe = action.fieldErrors;
  return (
    <Modal
      open
      onClose={onClose}
      title="New message"
      description="Only people you are allowed to message appear in the search."
      size="lg"
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>Cancel</Button>
          <Button type="submit" form="compose" loading={action.pending} disabled={recipients.length === 0 || !subject.trim() || !body.trim()}>
            Send
          </Button>
        </>
      }
    >
      <form
        id="compose"
        className="space-y-3"
        onSubmit={(e: FormEvent) => {
          e.preventDefault();
          void action.run();
        }}
      >
        <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />
        <RecipientPicker value={recipients} onChange={setRecipients} error={fe.recipient_ids} />
        <Input label="Subject" value={subject} onChange={(e) => setSubject(e.target.value)} error={fe.subject} required />
        <Textarea label="Message" rows={6} value={body} onChange={(e) => setBody(e.target.value)} error={fe.body} required />
      </form>
    </Modal>
  );
}
