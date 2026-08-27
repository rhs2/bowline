"use client";

import { useState, type FormEvent } from "react";
import { useParams } from "next/navigation";
import clsx from "clsx";
import { useMe } from "@/lib/me";
import { useQuery } from "@/lib/hooks";
import { api } from "@/lib/api";
import { useAction } from "@/lib/forms";
import { formatDateTime, humanize } from "@/lib/format";
import { isSupportAgent } from "@/lib/permissions";
import { ticketTransitions } from "@/lib/transitions";
import type { Recipient, TicketDetail, TicketStatus } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { Card, CardBody, CardHeader, DescriptionList } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { Textarea, FormError } from "@/components/ui/Field";
import { Modal } from "@/components/ui/Modal";
import { StatusBadge } from "@/components/StatusBadge";
import { SlaCountdown } from "@/components/SlaCountdown";
import { PageSkeleton } from "@/components/ui/Skeleton";
import { ErrorState } from "@/components/ui/States";
import { Avatar } from "@/components/ui/Avatar";
import { RecipientPicker } from "@/components/pickers/RecipientPicker";

export default function TicketPage() {
  const { id } = useParams<{ id: string }>();
  const { employee, permissions } = useMe();
  const ticket = useQuery<TicketDetail>(`support/tickets/${id}`);
  const [body, setBody] = useState("");
  const [assigning, setAssigning] = useState(false);

  const t = ticket.data;
  const agent = isSupportAgent(permissions);
  const requester = Boolean(t && employee && t.requester_id === employee.id);

  const reply = useAction(() => api.post(`support/tickets/${id}/messages`, { body }), {
    onSuccess: () => {
      setBody("");
      ticket.reload();
    },
  });
  const setStatus = useAction((status: TicketStatus) => api.post(`support/tickets/${id}/status`, { status }), {
    successMessage: "Status updated",
    onSuccess: () => ticket.reload(),
  });
  const rate = useAction((satisfaction: number) => api.post(`support/tickets/${id}/rate`, { satisfaction }), {
    successMessage: "Thanks for the feedback",
    onSuccess: () => ticket.reload(),
  });

  if (ticket.loading && !t) return <PageSkeleton />;
  if (ticket.error) {
    return (
      <div>
        <PageHeader title="Ticket" backHref="/support" backLabel="Support desk" />
        <ErrorState error={ticket.error} onRetry={ticket.reload} />
      </div>
    );
  }
  if (!t) return null;

  const transitions = ticketTransitions(t.status, { isAgent: agent, isRequester: requester, resolvedAt: t.resolved_at });
  const closedOrResolved = t.status === "resolved" || t.status === "closed";

  return (
    <div>
      <PageHeader
        title={t.subject}
        description={`${t.ticket_no}, opened ${formatDateTime(t.created_at)} by ${t.requester_name ?? "requester"}`}
        backHref="/support"
        backLabel="Support desk"
        meta={
          <>
            <StatusBadge status={t.status} />
            <StatusBadge status={t.priority} />
          </>
        }
        actions={
          <>
            {agent ? (
              <Button variant="secondary" onClick={() => setAssigning(true)}>
                {t.assignee_id ? "Reassign" : "Assign"}
              </Button>
            ) : null}
            {transitions.map((s) => (
              <Button
                key={s}
                variant={s === "resolved" || s === "closed" ? "success" : "secondary"}
                onClick={() => void setStatus.run(s)}
                loading={setStatus.pending}
              >
                {s === "open" && t.status === "resolved" ? "Reopen" : `Mark ${humanize(s).toLowerCase()}`}
              </Button>
            ))}
          </>
        }
      />

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
        <div className="space-y-4 lg:col-span-2">
          <Card>
            <CardHeader title="Conversation" />
            <CardBody className="space-y-4">
              {t.messages.map((m) => {
                const mine = m.sender_id === employee?.id;
                return (
                  <div key={m.id} className={clsx("flex gap-3", mine && "flex-row-reverse")}>
                    <Avatar name={m.sender_name ?? "?"} size="sm" />
                    <div className={clsx("max-w-[85%] rounded-lg px-3 py-2 text-sm", mine ? "bg-accent-50" : "bg-slate-100")}>
                      <p className="text-xs text-slate-500">
                        <span className="font-medium text-slate-700">{m.sender_name ?? "Unknown"}</span>, {formatDateTime(m.sent_at)}
                      </p>
                      <p className="mt-1 whitespace-pre-wrap text-slate-900">{m.body}</p>
                    </div>
                  </div>
                );
              })}
              {t.status !== "closed" ? (
                <form
                  onSubmit={(e: FormEvent) => {
                    e.preventDefault();
                    if (body.trim()) void reply.run();
                  }}
                  className="border-t border-slate-200 pt-4"
                >
                  <FormError message={reply.error?.message} />
                  <Textarea aria-label="Reply" rows={3} value={body} onChange={(e) => setBody(e.target.value)} placeholder="Add a reply" error={reply.fieldErrors.body} />
                  <div className="mt-2 flex justify-end">
                    <Button type="submit" loading={reply.pending} disabled={!body.trim()}>Send</Button>
                  </div>
                </form>
              ) : (
                <p className="border-t border-slate-200 pt-3 text-xs text-slate-500">This ticket is closed.</p>
              )}
            </CardBody>
          </Card>
        </div>

        <div className="space-y-4">
          <Card>
            <CardHeader title="Details" />
            <CardBody>
              <DescriptionList
                columns={1}
                items={[
                  { label: "Category", value: t.category === "it" ? "IT" : t.category === "hr" ? "HR" : humanize(t.category) },
                  { label: "Requester", value: t.requester_name },
                  { label: "Assignee", value: t.assignee_name ?? null },
                  { label: "First response SLA", value: <SlaCountdown ticket={t} /> },
                  { label: "SLA due", value: formatDateTime(t.sla_due_at) },
                  { label: "First response", value: t.first_response_at ? formatDateTime(t.first_response_at) : null },
                  { label: "Resolved", value: t.resolved_at ? formatDateTime(t.resolved_at) : null },
                  { label: "Closed", value: t.closed_at ? formatDateTime(t.closed_at) : null },
                ]}
              />
            </CardBody>
          </Card>
          {requester && closedOrResolved ? (
            <Card>
              <CardHeader title="How did we do?" description="Rate the help you received" />
              <CardBody>
                {t.satisfaction ? (
                  <p className="text-sm text-slate-700">You rated this ticket {t.satisfaction} out of 5. Thank you.</p>
                ) : (
                  <div className="flex gap-2">
                    {[1, 2, 3, 4, 5].map((n) => (
                      <Button key={n} variant="secondary" size="sm" onClick={() => void rate.run(n)} loading={rate.pending} aria-label={`Rate ${n} out of 5`}>
                        {n}
                      </Button>
                    ))}
                  </div>
                )}
              </CardBody>
            </Card>
          ) : null}
        </div>
      </div>

      {assigning ? (
        <AssignModal
          ticketId={t.id}
          onClose={() => setAssigning(false)}
          onDone={() => {
            setAssigning(false);
            ticket.reload();
          }}
        />
      ) : null}
    </div>
  );
}

function AssignModal({ ticketId, onClose, onDone }: { ticketId: string; onClose: () => void; onDone: () => void }) {
  const [assignee, setAssignee] = useState<Recipient[]>([]);
  const action = useAction(() => api.post(`support/tickets/${ticketId}/assign`, { assignee_id: assignee[0]?.id }), {
    successMessage: "Ticket assigned",
    onSuccess: onDone,
  });
  return (
    <Modal
      open
      onClose={onClose}
      title="Assign ticket"
      description="Assigning moves the ticket to triaged."
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>Cancel</Button>
          <Button onClick={() => void action.run()} loading={action.pending} disabled={assignee.length === 0}>Assign</Button>
        </>
      }
    >
      <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />
      <RecipientPicker label="Agent" value={assignee} onChange={setAssignee} multiple={false} error={action.fieldErrors.assignee_id} />
    </Modal>
  );
}
