"use client";

import { Suspense, useMemo, useState, type FormEvent } from "react";
import { useSearchParams } from "next/navigation";
import { useMe } from "@/lib/me";
import { useList, useQuery } from "@/lib/hooks";
import { api, asItems } from "@/lib/api";
import { useAction } from "@/lib/forms";
import { daysBetween, formatDate, formatNumber, humanize, todayIso } from "@/lib/format";
import { isLeaveApprover } from "@/lib/permissions";
import { leaveStatusOptions } from "@/lib/options";
import type { LeaveBalance, LeaveRequest, LeaveType, ListEnvelope } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { Card, CardBody } from "@/components/ui/Card";
import { DataTable, type Column } from "@/components/ui/DataTable";
import { FilterBar } from "@/components/ui/Filters";
import { Tabs } from "@/components/ui/Tabs";
import { Button } from "@/components/ui/Button";
import { FormError, Input, Select, Textarea } from "@/components/ui/Field";
import { Modal } from "@/components/ui/Modal";
import { StatusBadge } from "@/components/StatusBadge";
import { EmptyState, ErrorState } from "@/components/ui/States";
import { CardSkeleton, PageSkeleton } from "@/components/ui/Skeleton";

type Tab = "mine" | "approvals";

export default function LeavePage() {
  return (
    <Suspense fallback={<PageSkeleton />}>
      <Leave />
    </Suspense>
  );
}

function Leave() {
  const params = useSearchParams();
  const { permissions, has } = useMe();
  const approver = isLeaveApprover(permissions);
  const [tab, setTab] = useState<Tab>(params.get("tab") === "approvals" && approver ? "approvals" : "mine");
  const [requesting, setRequesting] = useState(params.get("tab") === "request");

  const types = useQuery<ListEnvelope<LeaveType> | LeaveType[]>("hr/leave/types");
  const typeRows = asItems(types.data);

  return (
    <div>
      <PageHeader
        title="Leave"
        description="Your balances and requests. Approvals are routed to your direct manager."
        actions={has("leave:request") ? <Button onClick={() => setRequesting(true)}>Request leave</Button> : null}
      />

      {approver ? (
        <div className="mb-4">
          <Tabs<Tab>
            tabs={[
              { key: "mine", label: "My leave" },
              { key: "approvals", label: "Approvals" },
            ]}
            value={tab}
            onChange={setTab}
          />
        </div>
      ) : null}

      {tab === "mine" ? <MyLeave types={typeRows} /> : <ApprovalQueue types={typeRows} />}

      {requesting ? (
        <RequestModal
          types={typeRows}
          onClose={() => setRequesting(false)}
          onCreated={() => {
            setRequesting(false);
            setTab("mine");
          }}
        />
      ) : null}
    </div>
  );
}

function typeName(types: LeaveType[], key: string): string {
  return types.find((t) => t.key === key)?.name ?? humanize(key);
}

// ---------------------------------------------------------------------------
// My leave
// ---------------------------------------------------------------------------

function MyLeave({ types }: { types: LeaveType[] }) {
  const { employee } = useMe();
  const me = employee?.id ?? null;
  const [status, setStatus] = useState("");
  // Both endpoints are scoped, so without an employee_id they return everyone the
  // caller may see: a manager would find their reports' balances and requests
  // listed here as if they were their own. This tab is only ever about the caller.
  const balances = useQuery<ListEnvelope<LeaveBalance> | LeaveBalance[]>(
    me ? "hr/leave/balances" : null,
    { query: { employee_id: me } },
  );
  const filters = useMemo(() => ({ status, employee_id: me ?? "" }), [status, me]);
  const list = useList<LeaveRequest>(me ? "hr/leave/requests" : null, filters);

  const cancel = useAction((id: string) => api.post(`hr/leave/requests/${id}/cancel`), {
    successMessage: "Request cancelled",
    onSuccess: () => {
      list.reload();
      balances.reload();
    },
  });

  const balanceRows = asItems(balances.data);

  const columns: Column<LeaveRequest>[] = [
    { key: "type", header: "Type", render: (r) => typeName(types, r.type_key) },
    {
      key: "dates",
      header: "Dates",
      render: (r) => (
        <span>
          {formatDate(r.start_date)} to {formatDate(r.end_date)}
        </span>
      ),
    },
    { key: "days", header: "Days", align: "right", render: (r) => formatNumber(r.days, 1) },
    { key: "status", header: "Status", render: (r) => <StatusBadge status={r.status} /> },
    { key: "reason", header: "Reason", render: (r) => r.reason ?? "", hideOnMobile: true },
    {
      key: "decision",
      header: "Decision",
      hideOnMobile: true,
      render: (r) =>
        r.decided_at ? (
          <span className="text-xs text-slate-600">
            {formatDate(r.decided_at)}
            {r.decision_note ? `, ${r.decision_note}` : ""}
          </span>
        ) : (
          <span className="text-xs text-slate-400">Pending</span>
        ),
    },
    {
      key: "actions",
      header: "",
      align: "right",
      render: (r) =>
        r.status === "pending" ? (
          <Button variant="secondary" size="sm" loading={cancel.pending} onClick={() => void cancel.run(r.id)}>
            Cancel
          </Button>
        ) : null,
    },
  ];

  return (
    <div className="space-y-6">
      <section aria-label="Balances">
        {balances.loading && balanceRows.length === 0 ? (
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
            <CardSkeleton lines={1} />
            <CardSkeleton lines={1} />
          </div>
        ) : balances.error ? (
          <ErrorState error={balances.error} onRetry={balances.reload} />
        ) : balanceRows.length === 0 ? (
          <Card>
            <CardBody>
              <EmptyState title="No balances yet" description="Balances are allocated at the start of the leave year." />
            </CardBody>
          </Card>
        ) : (
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
            {balanceRows.map((b) => {
              const allocated = Number(b.allocated) || 0;
              const used = Number(b.used) || 0;
              const remaining = allocated - used;
              const pct = allocated > 0 ? Math.min(100, Math.round((used / allocated) * 100)) : 0;
              return (
                <Card key={`${b.employee_id}-${b.type_key}-${b.year}`}>
                  <CardBody>
                    <p className="text-sm font-medium text-slate-500">{b.type_name ?? typeName(types, b.type_key)}</p>
                    <p className="mt-1 text-2xl font-semibold tracking-tight text-slate-900">
                      {formatNumber(remaining, 1)}
                      <span className="ml-1 text-sm font-normal text-slate-500">days left</span>
                    </p>
                    <div className="mt-3 h-1.5 w-full overflow-hidden rounded-full bg-slate-100">
                      <div className="h-full rounded-full bg-accent-500" style={{ width: `${pct}%` }} />
                    </div>
                    <p className="mt-2 text-xs text-slate-500">
                      {formatNumber(used, 1)} of {formatNumber(allocated, 1)} used in {b.year}
                    </p>
                  </CardBody>
                </Card>
              );
            })}
          </div>
        )}
      </section>

      <section aria-label="My requests">
        <FilterBar>
          <Select
            aria-label="Status"
            options={leaveStatusOptions}
            placeholder="Any status"
            value={status}
            onChange={(e) => setStatus(e.target.value)}
            className="w-full sm:w-48"
          />
        </FilterBar>
        <DataTable
          columns={columns}
          rows={list.items}
          rowKey={(r) => r.id}
          loading={list.loading}
          error={list.error}
          pagination={{ page: list.page, perPage: list.perPage, total: list.total, onPage: list.setPage }}
          empty={<EmptyState title="No leave requests" description="Requests you submit appear here." />}
        />
      </section>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Approvals
// ---------------------------------------------------------------------------

type Decision = "approve" | "reject";

function ApprovalQueue({ types }: { types: LeaveType[] }) {
  const filters = useMemo(() => ({ pending_for_me: 1 }), []);
  const list = useList<LeaveRequest>("hr/leave/requests", filters);
  const [decision, setDecision] = useState<{ request: LeaveRequest; kind: Decision } | null>(null);

  const columns: Column<LeaveRequest>[] = [
    { key: "who", header: "Employee", render: (r) => r.employee_name ?? r.employee_id },
    { key: "type", header: "Type", render: (r) => typeName(types, r.type_key) },
    {
      key: "dates",
      header: "Dates",
      render: (r) => (
        <span>
          {formatDate(r.start_date)} to {formatDate(r.end_date)}
        </span>
      ),
    },
    { key: "days", header: "Days", align: "right", render: (r) => formatNumber(r.days, 1) },
    { key: "reason", header: "Reason", render: (r) => r.reason ?? "", hideOnMobile: true },
    { key: "asked", header: "Requested", render: (r) => formatDate(r.created_at), hideOnMobile: true },
    {
      key: "actions",
      header: "",
      align: "right",
      render: (r) => (
        <span className="flex justify-end gap-2">
          <Button variant="success" size="sm" onClick={() => setDecision({ request: r, kind: "approve" })}>
            Approve
          </Button>
          <Button variant="secondary" size="sm" onClick={() => setDecision({ request: r, kind: "reject" })}>
            Reject
          </Button>
        </span>
      ),
    },
  ];

  return (
    <>
      <DataTable
        columns={columns}
        rows={list.items}
        rowKey={(r) => r.id}
        loading={list.loading}
        error={list.error}
        pagination={{ page: list.page, perPage: list.perPage, total: list.total, onPage: list.setPage }}
        empty={<EmptyState title="Nothing waiting on you" description="Requests from your reports appear here while they are pending." />}
      />
      {decision ? (
        <DecisionModal
          request={decision.request}
          kind={decision.kind}
          types={types}
          onClose={() => setDecision(null)}
          onDone={() => {
            setDecision(null);
            list.reload();
          }}
        />
      ) : null}
    </>
  );
}

function DecisionModal({
  request,
  kind,
  types,
  onClose,
  onDone,
}: {
  request: LeaveRequest;
  kind: Decision;
  types: LeaveType[];
  onClose: () => void;
  onDone: () => void;
}) {
  const [note, setNote] = useState("");
  const action = useAction(() => api.post(`hr/leave/requests/${request.id}/${kind}`, { note }), {
    successMessage: kind === "approve" ? "Leave approved" : "Leave rejected",
    onSuccess: onDone,
  });
  const noteRequired = kind === "reject";

  return (
    <Modal
      open
      onClose={onClose}
      title={kind === "approve" ? "Approve leave" : "Reject leave"}
      description={`${request.employee_name ?? "Employee"}, ${typeName(types, request.type_key)}, ${formatDate(request.start_date)} to ${formatDate(request.end_date)}`}
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant={kind === "approve" ? "success" : "danger"}
            loading={action.pending}
            disabled={noteRequired && !note.trim()}
            onClick={() => void action.run()}
          >
            {kind === "approve" ? "Approve" : "Reject"}
          </Button>
        </>
      }
    >
      <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />
      <Textarea
        label="Note"
        rows={3}
        value={note}
        onChange={(e) => setNote(e.target.value)}
        error={action.fieldErrors.note}
        required={noteRequired}
        hint={noteRequired ? "The requester sees this note." : "Optional. The requester sees this note."}
      />
      {kind === "approve" ? (
        <p className="mt-3 text-xs text-slate-500">Approving deducts {formatNumber(request.days, 1)} days from the balance.</p>
      ) : null}
    </Modal>
  );
}

// ---------------------------------------------------------------------------
// Request form
// ---------------------------------------------------------------------------

function RequestModal({
  types,
  onClose,
  onCreated,
}: {
  types: LeaveType[];
  onClose: () => void;
  onCreated: () => void;
}) {
  const today = todayIso();
  const [typeKey, setTypeKey] = useState(types[0]?.key ?? "annual");
  const [start, setStart] = useState(today);
  const [end, setEnd] = useState(today);
  const [reason, setReason] = useState("");

  const action = useAction(
    () =>
      api.post<LeaveRequest>("hr/leave/requests", {
        type_key: typeKey,
        start_date: start,
        end_date: end,
        reason,
      }),
    { successMessage: "Leave requested", onSuccess: onCreated },
  );

  const days = daysBetween(start, end);
  const rangeValid = days > 0;
  const fe = action.fieldErrors;

  return (
    <Modal
      open
      onClose={onClose}
      title="Request leave"
      description="Whole days only. The request goes to your direct manager."
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" form="leave-request" loading={action.pending} disabled={!rangeValid}>
            Submit request
          </Button>
        </>
      }
    >
      <form
        id="leave-request"
        className="space-y-3"
        onSubmit={(e: FormEvent) => {
          e.preventDefault();
          if (rangeValid) void action.run();
        }}
      >
        <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />
        <Select
          label="Type"
          options={types.map((t) => ({
            value: t.key,
            label: `${t.name}${t.paid ? "" : ", unpaid"}`,
          }))}
          value={typeKey}
          onChange={(e) => setTypeKey(e.target.value)}
          error={fe.type_key}
          required
        />
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <Input
            label="First day"
            type="date"
            value={start}
            onChange={(e) => {
              setStart(e.target.value);
              if (end < e.target.value) setEnd(e.target.value);
            }}
            error={fe.start_date}
            required
          />
          <Input
            label="Last day"
            type="date"
            value={end}
            min={start}
            onChange={(e) => setEnd(e.target.value)}
            error={fe.end_date ?? (rangeValid ? undefined : "The last day must not be before the first day")}
            required
          />
        </div>
        <p className="text-xs text-slate-500">
          {rangeValid ? `${days} calendar ${days === 1 ? "day" : "days"} requested.` : "Choose a valid date range."}
        </p>
        <Textarea
          label="Reason"
          rows={3}
          value={reason}
          onChange={(e) => setReason(e.target.value)}
          error={fe.reason}
          hint="Visible to your manager and to HR."
        />
      </form>
    </Modal>
  );
}
