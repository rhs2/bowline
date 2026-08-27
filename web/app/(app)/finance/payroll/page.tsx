"use client";

import { useMemo, useState } from "react";
import { useMe } from "@/lib/me";
import { useList, useQuery } from "@/lib/hooks";
import { api, asItems } from "@/lib/api";
import { useAction } from "@/lib/forms";
import { formatDateTime, formatMoney, periodLabel } from "@/lib/format";
import { payrollActions, type PayrollAction } from "@/lib/transitions";
import type { FiscalPeriod, ListEnvelope, PayrollItem, PayrollRun } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { Card, CardBody, CardHeader } from "@/components/ui/Card";
import { DataTable, type Column } from "@/components/ui/DataTable";
import { Button } from "@/components/ui/Button";
import { FormError, Select } from "@/components/ui/Field";
import { Modal } from "@/components/ui/Modal";
import { StatusBadge } from "@/components/StatusBadge";
import { EmptyState, ErrorState } from "@/components/ui/States";
import { Skeleton } from "@/components/ui/Skeleton";

const ACTION_LABELS: Record<PayrollAction, string> = { approve: "Approve", post: "Post to ledger" };

export default function PayrollPage() {
  const { permissions, has } = useMe();
  const [creating, setCreating] = useState(false);
  const [confirming, setConfirming] = useState<{ run: PayrollRun; action: PayrollAction } | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);

  const filters = useMemo(() => ({}), []);
  const list = useList<PayrollRun>("finance/payroll/runs", filters);
  const canPrepare = has("payroll:prepare");

  function periodOf(run: PayrollRun): string {
    return run.period ? periodLabel(run.period.year, run.period.month) : run.period_id;
  }

  const columns: Column<PayrollRun>[] = [
    { key: "period", header: "Period", render: (r) => <span className="font-medium text-slate-900">{periodOf(r)}</span> },
    { key: "status", header: "Status", render: (r) => <StatusBadge status={r.status} /> },
    { key: "gross", header: "Gross", align: "right", render: (r) => formatMoney(r.total_gross) },
    { key: "deductions", header: "Deductions", align: "right", render: (r) => formatMoney(r.total_deductions), hideOnMobile: true },
    { key: "net", header: "Net", align: "right", render: (r) => formatMoney(r.total_net) },
    {
      key: "approved",
      header: "Approved",
      hideOnMobile: true,
      render: (r) => (r.approved_at ? formatDateTime(r.approved_at) : ""),
    },
    { key: "posted", header: "Posted", hideOnMobile: true, render: (r) => (r.posted_at ? formatDateTime(r.posted_at) : "") },
    {
      key: "actions",
      header: "",
      align: "right",
      render: (r) => (
        <span className="flex flex-wrap justify-end gap-2">
          <Button variant="ghost" size="sm" onClick={() => setExpanded(expanded === r.id ? null : r.id)}>
            {expanded === r.id ? "Hide items" : "Items"}
          </Button>
          {payrollActions(r.status, permissions).map((a) => (
            <Button key={a} size="sm" variant={a === "post" ? "success" : "primary"} onClick={() => setConfirming({ run: r, action: a })}>
              {ACTION_LABELS[a]}
            </Button>
          ))}
        </span>
      ),
    },
  ];

  const expandedRun = list.items.find((r) => r.id === expanded) ?? null;

  return (
    <div>
      <PageHeader
        title="Payroll"
        description="One run per period, one item per active employee. Posting writes Salaries against Salaries Payable."
        actions={canPrepare ? <Button onClick={() => setCreating(true)}>New run</Button> : null}
      />

      <DataTable
        columns={columns}
        rows={list.items}
        rowKey={(r) => r.id}
        loading={list.loading}
        error={list.error}
        pagination={{ page: list.page, perPage: list.perPage, total: list.total, onPage: list.setPage }}
        empty={<EmptyState title="No payroll runs" description="Runs are prepared once a fiscal period is ready." />}
      />

      {expandedRun ? (
        <div className="mt-4">
          <RunItems run={expandedRun} periodName={periodOf(expandedRun)} />
        </div>
      ) : null}

      {creating ? (
        <NewRunModal
          onClose={() => setCreating(false)}
          onCreated={() => {
            setCreating(false);
            list.reload();
          }}
        />
      ) : null}
      {confirming ? (
        <ConfirmModal
          run={confirming.run}
          action={confirming.action}
          periodName={periodOf(confirming.run)}
          onClose={() => setConfirming(null)}
          onDone={() => {
            setConfirming(null);
            list.reload();
          }}
        />
      ) : null}
    </div>
  );
}

function RunItems({ run, periodName }: { run: PayrollRun; periodName: string }) {
  const detail = useQuery<PayrollRun>(run.items ? null : `finance/payroll/runs/${run.id}`);
  const items = run.items ?? detail.data?.items ?? [];

  const columns: Column<PayrollItem>[] = [
    { key: "employee", header: "Employee", render: (i) => i.employee_name ?? i.employee_id },
    { key: "gross", header: "Gross", align: "right", render: (i) => formatMoney(i.gross) },
    { key: "deductions", header: "Deductions", align: "right", render: (i) => formatMoney(i.deductions) },
    { key: "net", header: "Net", align: "right", render: (i) => formatMoney(i.net) },
  ];

  return (
    <Card>
      <CardHeader title={`Items for ${periodName}`} description={`${items.length} employees in this run`} />
      <CardBody className="px-0 py-0">
        {detail.loading && items.length === 0 ? (
          <div className="space-y-2 p-4">
            <Skeleton className="h-4 w-full" />
            <Skeleton className="h-4 w-5/6" />
          </div>
        ) : detail.error ? (
          <div className="p-4">
            <ErrorState error={detail.error} onRetry={detail.reload} />
          </div>
        ) : (
          <DataTable
            columns={columns}
            rows={items}
            rowKey={(i) => i.id}
            dense
            empty={<EmptyState title="No items" description="This run has no employee items." />}
          />
        )}
      </CardBody>
    </Card>
  );
}

function NewRunModal({ onClose, onCreated }: { onClose: () => void; onCreated: () => void }) {
  const periods = useQuery<ListEnvelope<FiscalPeriod> | FiscalPeriod[]>("finance/periods");
  const [periodId, setPeriodId] = useState("");
  const action = useAction(() => api.post<PayrollRun>("finance/payroll/runs", { period_id: periodId }), {
    successMessage: "Payroll run created as a draft",
    onSuccess: onCreated,
  });

  const options = [...asItems(periods.data)]
    .filter((p) => p.status === "open")
    .sort((a, b) => b.year - a.year || b.month - a.month)
    .map((p) => ({ value: p.id, label: periodLabel(p.year, p.month) }));

  return (
    <Modal
      open
      onClose={onClose}
      title="New payroll run"
      description="Creates one item per active employee from their base salary. The run starts as a draft."
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button loading={action.pending} disabled={!periodId} onClick={() => void action.run()}>
            Create run
          </Button>
        </>
      }
    >
      <div className="space-y-3">
        <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />
        <Select
          label="Period"
          options={options}
          placeholder={options.length === 0 ? "No open periods" : "Choose a period"}
          value={periodId}
          onChange={(e) => setPeriodId(e.target.value)}
          error={action.fieldErrors.period_id}
          required
        />
        <p className="text-xs text-slate-500">Closed periods cannot take a new run.</p>
      </div>
    </Modal>
  );
}

function ConfirmModal({
  run,
  action,
  periodName,
  onClose,
  onDone,
}: {
  run: PayrollRun;
  action: PayrollAction;
  periodName: string;
  onClose: () => void;
  onDone: () => void;
}) {
  const call = useAction(() => api.post(`finance/payroll/runs/${run.id}/${action}`), {
    successMessage: action === "approve" ? "Payroll run approved" : "Payroll run posted",
    onSuccess: onDone,
  });

  return (
    <Modal
      open
      onClose={onClose}
      title={`${ACTION_LABELS[action]}: ${periodName}`}
      description={
        action === "approve"
          ? "Approving locks the amounts so the run can be posted."
          : "Posting writes Salaries against Salaries Payable in the general ledger. This cannot be undone except by a reversing entry."
      }
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button variant={action === "post" ? "success" : "primary"} loading={call.pending} onClick={() => void call.run()}>
            {ACTION_LABELS[action]}
          </Button>
        </>
      }
    >
      <FormError message={call.error?.message} />
      <dl className="grid grid-cols-3 gap-3 text-sm">
        <div>
          <dt className="text-xs uppercase tracking-wide text-slate-500">Gross</dt>
          <dd className="mt-0.5 font-semibold tabular-nums text-slate-900">{formatMoney(run.total_gross)}</dd>
        </div>
        <div>
          <dt className="text-xs uppercase tracking-wide text-slate-500">Deductions</dt>
          <dd className="mt-0.5 font-semibold tabular-nums text-slate-900">{formatMoney(run.total_deductions)}</dd>
        </div>
        <div>
          <dt className="text-xs uppercase tracking-wide text-slate-500">Net</dt>
          <dd className="mt-0.5 font-semibold tabular-nums text-slate-900">{formatMoney(run.total_net)}</dd>
        </div>
      </dl>
    </Modal>
  );
}
