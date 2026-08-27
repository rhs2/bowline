"use client";

import { Suspense, useMemo, useState, type FormEvent } from "react";
import { useSearchParams } from "next/navigation";
import { useMe } from "@/lib/me";
import { useList } from "@/lib/hooks";
import { api } from "@/lib/api";
import { useAction } from "@/lib/forms";
import { formatDate, formatDateTime, formatMoney, humanize, todayIso } from "@/lib/format";
import { addAmounts } from "@/lib/ledger";
import { isExpenseApprover } from "@/lib/permissions";
import { expenseCategoryOptions, expenseStatusOptions } from "@/lib/options";
import { expenseActions, expensePendingStep, expenseStepLabel, isMyExpenseStep } from "@/lib/transitions";
import type { Expense, ExpenseStatus } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { Card, CardBody, CardHeader } from "@/components/ui/Card";
import { DataTable, type Column } from "@/components/ui/DataTable";
import { FilterBar } from "@/components/ui/Filters";
import { Tabs } from "@/components/ui/Tabs";
import { Button } from "@/components/ui/Button";
import { FormError, Input, Select, Textarea } from "@/components/ui/Field";
import { Modal } from "@/components/ui/Modal";
import { StatusBadge } from "@/components/StatusBadge";
import { Badge } from "@/components/ui/Badge";
import { EmptyState } from "@/components/ui/States";
import { PageSkeleton } from "@/components/ui/Skeleton";

type Tab = "mine" | "queue";
type Decision = "approve" | "reject" | "pay";

const DECISION_LABELS: Record<Decision, string> = {
  approve: "Approve",
  reject: "Reject",
  pay: "Mark paid",
};

/** The two-step chain from docs/DOMAIN.md, rendered as a progress trail. */
function StepTrail({ status }: { status: ExpenseStatus }) {
  const step = expensePendingStep(status);
  const steps: Array<{ key: string; label: string; state: "done" | "current" | "todo" }> = [
    {
      key: "manager",
      label: "Manager",
      state: step === "manager" ? "current" : status === "rejected" ? "todo" : "done",
    },
    {
      key: "finance",
      label: "Finance",
      state: step === "finance" ? "current" : step === "manager" || status === "rejected" ? "todo" : "done",
    },
    {
      key: "paid",
      label: "Paid",
      state: status === "paid" ? "done" : step === "payment" ? "current" : "todo",
    },
  ];
  return (
    <span className="flex flex-wrap items-center gap-1">
      {steps.map((s) => (
        <Badge key={s.key} tone={s.state === "done" ? "success" : s.state === "current" ? "info" : "neutral"}>
          {s.label}
        </Badge>
      ))}
    </span>
  );
}

export default function ExpensesPage() {
  return (
    <Suspense fallback={<PageSkeleton />}>
      <Expenses />
    </Suspense>
  );
}

function Expenses() {
  const params = useSearchParams();
  const { permissions, has } = useMe();
  const approver = isExpenseApprover(permissions);
  const [tab, setTab] = useState<Tab>(params.get("tab") === "queue" && approver ? "queue" : "mine");
  const [submitting, setSubmitting] = useState(params.get("tab") === "new");

  return (
    <div>
      <PageHeader
        title="Expenses"
        description="Claims go to your manager first, then to finance, and are paid from the ledger."
        actions={has("expenses:submit") ? <Button onClick={() => setSubmitting(true)}>Submit a claim</Button> : null}
      />

      {approver ? (
        <div className="mb-4">
          <Tabs<Tab>
            tabs={[
              { key: "mine", label: "My claims" },
              { key: "queue", label: "Approvals" },
            ]}
            value={tab}
            onChange={setTab}
          />
        </div>
      ) : null}

      {tab === "mine" ? <MyClaims /> : <ApprovalQueue />}

      {submitting ? (
        <SubmitModal
          onClose={() => setSubmitting(false)}
          onCreated={() => {
            setSubmitting(false);
            setTab("mine");
          }}
        />
      ) : null}
    </div>
  );
}

function MyClaims() {
  const [status, setStatus] = useState("");
  const filters = useMemo(() => ({ status }), [status]);
  const list = useList<Expense>("finance/expenses", filters);

  const columns: Column<Expense>[] = [
    { key: "date", header: "Incurred", render: (e) => formatDate(e.incurred_on) },
    { key: "category", header: "Category", render: (e) => humanize(e.category) },
    { key: "description", header: "Description", render: (e) => e.description },
    { key: "amount", header: "Amount", align: "right", render: (e) => formatMoney(e.amount, e.currency) },
    { key: "status", header: "Status", render: (e) => <StatusBadge status={e.status} /> },
    { key: "step", header: "Progress", render: (e) => <StepTrail status={e.status} />, hideOnMobile: true },
    {
      key: "note",
      header: "Note",
      hideOnMobile: true,
      render: (e) => (e.rejection_note ? <span className="text-xs text-red-700">{e.rejection_note}</span> : ""),
    },
  ];

  const outstanding = list.items
    .filter((e) => e.status !== "rejected" && e.status !== "paid")
    .reduce<string>((acc, e) => addAmounts(acc, e.amount), "0.00");

  return (
    <div>
      <FilterBar>
        <Select
          aria-label="Status"
          options={expenseStatusOptions}
          placeholder="Any status"
          value={status}
          onChange={(e) => setStatus(e.target.value)}
          className="w-full sm:w-52"
        />
      </FilterBar>
      <DataTable
        columns={columns}
        rows={list.items}
        rowKey={(e) => e.id}
        loading={list.loading}
        error={list.error}
        pagination={{ page: list.page, perPage: list.perPage, total: list.total, onPage: list.setPage }}
        empty={<EmptyState title="No claims" description="Expenses you submit appear here with their approval progress." />}
      />
      {list.items.length > 0 ? (
        <p className="mt-2 text-xs text-slate-500">
          {formatMoney(outstanding)} awaiting approval or payment across the claims on this page.
        </p>
      ) : null}
    </div>
  );
}

function ApprovalQueue() {
  const { permissions } = useMe();
  const filters = useMemo(() => ({ pending_for_me: 1 }), []);
  const list = useList<Expense>("finance/expenses", filters);
  const [decision, setDecision] = useState<{ expense: Expense; kind: Decision } | null>(null);

  const columns: Column<Expense>[] = [
    { key: "who", header: "Employee", render: (e) => e.employee_name ?? e.employee_id },
    { key: "date", header: "Incurred", render: (e) => formatDate(e.incurred_on) },
    { key: "category", header: "Category", render: (e) => humanize(e.category) },
    { key: "description", header: "Description", render: (e) => e.description, hideOnMobile: true },
    { key: "amount", header: "Amount", align: "right", render: (e) => formatMoney(e.amount, e.currency) },
    {
      key: "step",
      header: "Waiting on",
      render: (e) => (
        <span className="flex flex-col gap-1">
          <span className="text-xs text-slate-600">{expenseStepLabel(e.status)}</span>
          <StepTrail status={e.status} />
        </span>
      ),
    },
    { key: "submitted", header: "Submitted", render: (e) => formatDateTime(e.created_at), hideOnMobile: true },
    {
      key: "actions",
      header: "",
      align: "right",
      render: (e) => {
        const available = expenseActions(e.status, permissions);
        if (!isMyExpenseStep(e.status, permissions) || available.length === 0) {
          return <span className="text-xs text-slate-400">Not your step</span>;
        }
        return (
          <span className="flex justify-end gap-2">
            {available.map((a) => (
              <Button
                key={a}
                size="sm"
                variant={a === "reject" ? "secondary" : a === "pay" ? "success" : "primary"}
                onClick={() => setDecision({ expense: e, kind: a })}
              >
                {DECISION_LABELS[a]}
              </Button>
            ))}
          </span>
        );
      },
    },
  ];

  const total = list.items.reduce<string>((acc, e) => addAmounts(acc, e.amount), "0.00");

  return (
    <div>
      <Card className="mb-4">
        <CardHeader
          title="Waiting on you"
          description="Claims are approved by the manager first, then by finance, and only finance can mark them paid."
        />
        <CardBody className="py-3">
          <p className="text-sm text-slate-700">
            {list.total} {list.total === 1 ? "claim" : "claims"}, {formatMoney(total)} on this page.
          </p>
        </CardBody>
      </Card>
      <DataTable
        columns={columns}
        rows={list.items}
        rowKey={(e) => e.id}
        loading={list.loading}
        error={list.error}
        pagination={{ page: list.page, perPage: list.perPage, total: list.total, onPage: list.setPage }}
        empty={<EmptyState title="Nothing waiting on you" description="Claims appear here when they reach your step." />}
      />
      {decision ? (
        <DecisionModal
          expense={decision.expense}
          kind={decision.kind}
          onClose={() => setDecision(null)}
          onDone={() => {
            setDecision(null);
            list.reload();
          }}
        />
      ) : null}
    </div>
  );
}

function DecisionModal({
  expense,
  kind,
  onClose,
  onDone,
}: {
  expense: Expense;
  kind: Decision;
  onClose: () => void;
  onDone: () => void;
}) {
  const [note, setNote] = useState("");
  const action = useAction(
    () => api.post(`finance/expenses/${expense.id}/${kind}`, kind === "reject" ? { note } : { note: note || undefined }),
    {
      successMessage:
        kind === "approve" ? "Claim approved" : kind === "reject" ? "Claim rejected" : "Claim marked paid",
      onSuccess: onDone,
    },
  );
  const noteRequired = kind === "reject";

  const explanation =
    kind === "pay"
      ? "Paying posts the expense against Cash and closes the claim."
      : kind === "approve"
        ? `Approving moves the claim to the next step: ${expenseStepLabel(expense.status)} now, finance after you.`
        : "Rejecting closes the claim. The employee sees your note.";

  return (
    <Modal
      open
      onClose={onClose}
      title={`${DECISION_LABELS[kind]}: ${formatMoney(expense.amount, expense.currency)}`}
      description={`${expense.employee_name ?? "Employee"}, ${humanize(expense.category)}, ${formatDate(expense.incurred_on)}`}
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant={kind === "reject" ? "danger" : kind === "pay" ? "success" : "primary"}
            loading={action.pending}
            disabled={noteRequired && !note.trim()}
            onClick={() => void action.run()}
          >
            {DECISION_LABELS[kind]}
          </Button>
        </>
      }
    >
      <div className="space-y-3">
        <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />
        <p className="text-sm text-slate-600">{explanation}</p>
        <p className="rounded-md bg-slate-50 px-3 py-2 text-sm text-slate-700">{expense.description}</p>
        <Textarea
          label="Note"
          rows={3}
          value={note}
          onChange={(e) => setNote(e.target.value)}
          error={action.fieldErrors.note}
          required={noteRequired}
          hint={noteRequired ? "Required. The employee sees this." : "Optional."}
        />
      </div>
    </Modal>
  );
}

function SubmitModal({ onClose, onCreated }: { onClose: () => void; onCreated: () => void }) {
  const [form, setForm] = useState({
    category: "travel",
    amount: "",
    currency: "USD",
    incurred_on: todayIso(),
    description: "",
    receipt_s3_key: "",
  });

  const action = useAction(
    () =>
      api.post<Expense>("finance/expenses", {
        category: form.category,
        amount: form.amount,
        currency: form.currency,
        incurred_on: form.incurred_on,
        description: form.description,
        receipt_s3_key: form.receipt_s3_key || undefined,
      }),
    { successMessage: "Claim submitted to your manager", onSuccess: onCreated },
  );

  function set<K extends keyof typeof form>(key: K, value: string) {
    setForm((f) => ({ ...f, [key]: value }));
  }

  const fe = action.fieldErrors;
  const ready = Boolean(form.amount && form.description.trim());

  return (
    <Modal
      open
      onClose={onClose}
      title="Submit an expense claim"
      description="Your manager approves first, then finance."
      size="lg"
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" form="submit-expense" loading={action.pending} disabled={!ready}>
            Submit claim
          </Button>
        </>
      }
    >
      <form
        id="submit-expense"
        className="space-y-3"
        onSubmit={(e: FormEvent) => {
          e.preventDefault();
          if (ready) void action.run();
        }}
      >
        <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <Select
            label="Category"
            options={expenseCategoryOptions}
            value={form.category}
            onChange={(e) => set("category", e.target.value)}
            error={fe.category}
            required
          />
          <Input label="Incurred on" type="date" value={form.incurred_on} onChange={(e) => set("incurred_on", e.target.value)} error={fe.incurred_on} required />
          <Input label="Amount" inputMode="decimal" value={form.amount} onChange={(e) => set("amount", e.target.value)} error={fe.amount} required />
          <Input label="Currency" maxLength={3} value={form.currency} onChange={(e) => set("currency", e.target.value.toUpperCase())} error={fe.currency} />
        </div>
        <Textarea
          label="Description"
          rows={3}
          value={form.description}
          onChange={(e) => set("description", e.target.value)}
          error={fe.description}
          hint="What it was for. Approvers read this before deciding."
          required
        />
        <Input
          label="Receipt key"
          value={form.receipt_s3_key}
          onChange={(e) => set("receipt_s3_key", e.target.value)}
          error={fe.receipt_s3_key}
          hint="Optional storage key for a receipt that was uploaded elsewhere."
        />
      </form>
    </Modal>
  );
}
