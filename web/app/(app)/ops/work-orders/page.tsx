"use client";

import { useMemo, useState } from "react";
import Link from "next/link";
import clsx from "clsx";
import { useMe } from "@/lib/me";
import { useList, useNow } from "@/lib/hooks";
import { api } from "@/lib/api";
import { useAction } from "@/lib/forms";
import { formatDateTime, formatRelative, humanize } from "@/lib/format";
import { workOrderTransitions } from "@/lib/transitions";
import type { WorkOrder, WorkOrderStatus } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { Card, CardBody } from "@/components/ui/Card";
import { Chips } from "@/components/ui/Filters";
import { Tabs } from "@/components/ui/Tabs";
import { Button } from "@/components/ui/Button";
import { FormError, Textarea } from "@/components/ui/Field";
import { Modal } from "@/components/ui/Modal";
import { StatusBadge } from "@/components/StatusBadge";
import { Badge } from "@/components/ui/Badge";
import { EmptyState, ErrorState } from "@/components/ui/States";
import { CardSkeleton } from "@/components/ui/Skeleton";

type Tab = "mine" | "team";

const OPEN_STATUSES: WorkOrderStatus[] = ["open", "in_progress", "blocked"];

const STATUS_CHIPS: Array<{ value: WorkOrderStatus; label: string }> = [
  { value: "open", label: "Open" },
  { value: "in_progress", label: "In progress" },
  { value: "blocked", label: "Blocked" },
  { value: "done", label: "Done" },
];

/** Wording that reads like an instruction on a phone, not a state name. */
function actionLabel(to: WorkOrderStatus): string {
  switch (to) {
    case "in_progress":
      return "Start";
    case "done":
      return "Done";
    case "blocked":
      return "Blocked";
    default:
      return humanize(to);
  }
}

export default function WorkOrdersPage() {
  const { can, has } = useMe();
  const canManage = can("tasks:manage:subtree");
  const [tab, setTab] = useState<Tab>("mine");
  const [status, setStatus] = useState<WorkOrderStatus | "">("");
  const [acting, setActing] = useState<{ order: WorkOrder; to: WorkOrderStatus } | null>(null);

  const filters = useMemo(
    () => ({ mine: tab === "mine" ? 1 : undefined, status: status || undefined }),
    [tab, status],
  );
  const list = useList<WorkOrder>("ops/work-orders", filters, { perPage: 50 });

  const rows = list.items;
  const openCount = rows.filter((w) => OPEN_STATUSES.includes(w.status)).length;

  if (!has("tasks:read:self") && !canManage) {
    return (
      <div>
        <PageHeader title="Work orders" />
        <Card>
          <CardBody>
            <EmptyState title="No access" description="Work orders are assigned to ground staff and their supervisors." />
          </CardBody>
        </Card>
      </div>
    );
  }

  return (
    <div>
      <PageHeader
        title="Work orders"
        description={tab === "mine" ? "Tasks assigned to you. Tap a button to update the crew." : "Tasks across your team."}
        meta={openCount > 0 ? <Badge tone="accent">{openCount} to do</Badge> : null}
      />

      {canManage ? (
        <div className="mb-4">
          <Tabs<Tab>
            tabs={[
              { key: "mine", label: "My tasks" },
              { key: "team", label: "My team" },
            ]}
            value={tab}
            onChange={setTab}
          />
        </div>
      ) : null}

      <div className="mb-4">
        <Chips options={STATUS_CHIPS} value={status} onChange={setStatus} allLabel="All" />
      </div>

      {list.error ? (
        <ErrorState error={list.error} onRetry={list.reload} />
      ) : list.loading && rows.length === 0 ? (
        <div className="space-y-3">
          <CardSkeleton lines={2} />
          <CardSkeleton lines={2} />
        </div>
      ) : rows.length === 0 ? (
        <Card>
          <CardBody>
            <EmptyState
              title="Nothing to do"
              description={tab === "mine" ? "You have no work orders in this state." : "Your team has no work orders in this state."}
            />
          </CardBody>
        </Card>
      ) : (
        <ul className="space-y-3">
          {rows.map((w) => (
            <li key={w.id}>
              <WorkOrderCard order={w} showAssignee={tab === "team"} onAct={(to) => setActing({ order: w, to })} />
            </li>
          ))}
        </ul>
      )}

      {list.total > rows.length ? (
        <p className="mt-4 text-center text-sm text-slate-500">
          Showing {rows.length} of {list.total}. Use the filters to narrow the list.
        </p>
      ) : null}

      {acting ? (
        <StatusModal
          order={acting.order}
          to={acting.to}
          onClose={() => setActing(null)}
          onDone={() => {
            setActing(null);
            list.reload();
          }}
        />
      ) : null}
    </div>
  );
}

function WorkOrderCard({
  order,
  showAssignee,
  onAct,
}: {
  order: WorkOrder;
  showAssignee: boolean;
  onAct: (to: WorkOrderStatus) => void;
}) {
  const now = useNow(60000);
  const nextStates = workOrderTransitions(order.status);
  const overdue =
    order.due_at !== null && OPEN_STATUSES.includes(order.status) && new Date(order.due_at).getTime() < now.getTime();

  return (
    <Card className={clsx(overdue && "border-amber-300")}>
      <CardBody className="space-y-3">
        <div className="flex flex-wrap items-start justify-between gap-2">
          <div className="min-w-0">
            <p className="text-base font-semibold text-slate-900">{order.title}</p>
            <p className="mt-0.5 text-sm text-slate-600">
              {humanize(order.kind)}
              {order.site_name ? `, ${order.site_name}` : ""}
              {showAssignee && order.assigned_to_name ? `, ${order.assigned_to_name}` : ""}
            </p>
          </div>
          <div className="flex shrink-0 flex-col items-end gap-1">
            <StatusBadge status={order.status} />
            {overdue ? <Badge tone="warning">Overdue</Badge> : null}
          </div>
        </div>

        {order.instructions ? (
          <p className="whitespace-pre-wrap rounded-md bg-slate-50 px-3 py-2 text-sm text-slate-700">{order.instructions}</p>
        ) : null}

        <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-slate-500">
          {order.due_at ? (
            <span className={clsx(overdue && "font-medium text-amber-700")}>
              Due {formatDateTime(order.due_at)}, {formatRelative(order.due_at, now)}
            </span>
          ) : (
            <span>No due time</span>
          )}
          {order.shipment_id ? (
            <Link href={`/ops/shipments/${order.shipment_id}`} className="font-medium text-accent-700 hover:underline">
              {order.shipment_reference ?? "Shipment"}
            </Link>
          ) : null}
          {order.started_at ? <span>Started {formatDateTime(order.started_at)}</span> : null}
          {order.completed_at ? <span>Finished {formatDateTime(order.completed_at)}</span> : null}
        </div>

        {order.notes ? <p className="text-sm text-slate-600">Last note: {order.notes}</p> : null}

        {nextStates.length > 0 ? (
          <div className="grid grid-cols-1 gap-2 pt-1 sm:grid-cols-3">
            {nextStates.map((to) => (
              <Button
                key={to}
                size="lg"
                block
                variant={to === "done" ? "success" : to === "blocked" ? "secondary" : "primary"}
                onClick={() => onAct(to)}
              >
                {actionLabel(to)}
              </Button>
            ))}
          </div>
        ) : (
          <p className="text-sm text-slate-500">This task is closed.</p>
        )}
      </CardBody>
    </Card>
  );
}

function StatusModal({
  order,
  to,
  onClose,
  onDone,
}: {
  order: WorkOrder;
  to: WorkOrderStatus;
  onClose: () => void;
  onDone: () => void;
}) {
  const [notes, setNotes] = useState("");
  const action = useAction(
    () => api.post(`ops/work-orders/${order.id}/status`, { status: to, notes: notes || undefined }),
    { successMessage: `Marked ${actionLabel(to).toLowerCase()}`, onSuccess: onDone },
  );
  const notesRequired = to === "blocked";

  return (
    <Modal
      open
      onClose={onClose}
      title={`${actionLabel(to)}: ${order.title}`}
      description={notesRequired ? "Say what is blocking the task so a supervisor can clear it." : "Add a note for the record. Optional."}
      footer={
        <>
          <Button variant="secondary" size="lg" onClick={onClose}>
            Cancel
          </Button>
          <Button
            size="lg"
            variant={to === "done" ? "success" : "primary"}
            loading={action.pending}
            disabled={notesRequired && !notes.trim()}
            onClick={() => void action.run()}
          >
            Confirm
          </Button>
        </>
      }
    >
      <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />
      <Textarea
        label="Notes"
        rows={4}
        value={notes}
        onChange={(e) => setNotes(e.target.value)}
        error={action.fieldErrors.notes}
        required={notesRequired}
      />
    </Modal>
  );
}
