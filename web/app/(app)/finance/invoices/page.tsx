"use client";

import { Suspense, useMemo, useState, type FormEvent } from "react";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { useMe } from "@/lib/me";
import { useDebounced, useList } from "@/lib/hooks";
import { api } from "@/lib/api";
import { useAction } from "@/lib/forms";
import { formatDate, formatMoney, formatNumber, todayIso } from "@/lib/format";
import { addAmounts, subtractAmounts, toCents } from "@/lib/ledger";
import { invoiceStatusOptions } from "@/lib/options";
import type { Invoice } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { Card, CardBody } from "@/components/ui/Card";
import { DataTable, type Column } from "@/components/ui/DataTable";
import { FilterBar, SearchInput } from "@/components/ui/Filters";
import { Button } from "@/components/ui/Button";
import { Checkbox, FormError, Input, Select, Textarea } from "@/components/ui/Field";
import { Modal } from "@/components/ui/Modal";
import { StatusBadge } from "@/components/StatusBadge";
import { Badge } from "@/components/ui/Badge";
import { EmptyState } from "@/components/ui/States";
import { PageSkeleton } from "@/components/ui/Skeleton";
import { CustomerPicker, type CustomerOption } from "@/components/pickers/CustomerPicker";

interface CurrencyTotals {
  currency: string;
  total: string;
  paid: string;
  outstanding: string;
  count: number;
}

/** Page totals by currency. Invoices are not converted, so each currency stands alone. */
function totalsByCurrency(rows: Invoice[]): CurrencyTotals[] {
  const map = new Map<string, CurrencyTotals>();
  for (const row of rows) {
    const current = map.get(row.currency) ?? {
      currency: row.currency,
      total: "0.00",
      paid: "0.00",
      outstanding: "0.00",
      count: 0,
    };
    current.total = addAmounts(current.total, row.total);
    current.paid = addAmounts(current.paid, row.amount_paid);
    current.outstanding = subtractAmounts(current.total, current.paid);
    current.count += 1;
    map.set(row.currency, current);
  }
  return [...map.values()];
}

function isOverdue(inv: Invoice, today: string): boolean {
  if (!inv.due_date) return false;
  if (inv.status !== "issued" && inv.status !== "partially_paid") return false;
  return inv.due_date < today && toCents(inv.total) > toCents(inv.amount_paid);
}

export default function InvoicesPage() {
  return (
    <Suspense fallback={<PageSkeleton />}>
      <Invoices />
    </Suspense>
  );
}

function Invoices() {
  const params = useSearchParams();
  const router = useRouter();
  const { has } = useMe();
  const today = todayIso();
  const [q, setQ] = useState("");
  const [status, setStatus] = useState("");
  const [customer, setCustomer] = useState<CustomerOption | null>(null);
  const [overdue, setOverdue] = useState(params.get("overdue") === "1");
  const [creating, setCreating] = useState(params.get("new") === "1");
  const debouncedQ = useDebounced(q);

  const filters = useMemo(
    () => ({ q: debouncedQ, status, customer_id: customer?.id ?? "", overdue: overdue ? 1 : undefined }),
    [debouncedQ, status, customer, overdue],
  );
  const list = useList<Invoice>("finance/invoices", filters);
  const totals = totalsByCurrency(list.items);

  const columns: Column<Invoice>[] = [
    {
      key: "no",
      header: "Invoice",
      render: (i) => (
        <Link href={`/finance/invoices/${i.id}`} className="font-mono text-xs font-semibold text-slate-900 hover:text-accent-700">
          {i.invoice_no}
        </Link>
      ),
    },
    { key: "customer", header: "Customer", render: (i) => i.customer_name ?? "" },
    {
      key: "shipment",
      header: "Shipment",
      hideOnMobile: true,
      render: (i) =>
        i.shipment_id ? (
          <Link href={`/ops/shipments/${i.shipment_id}`} className="font-mono text-xs hover:text-accent-700">
            {i.shipment_reference ?? "Shipment"}
          </Link>
        ) : (
          ""
        ),
    },
    { key: "issued", header: "Issued", render: (i) => (i.issue_date ? formatDate(i.issue_date) : ""), hideOnMobile: true },
    {
      key: "due",
      header: "Due",
      render: (i) => (
        <span className={isOverdue(i, today) ? "font-medium text-red-700" : undefined}>
          {i.due_date ? formatDate(i.due_date) : ""}
        </span>
      ),
    },
    { key: "total", header: "Total", align: "right", render: (i) => formatMoney(i.total, i.currency) },
    {
      key: "outstanding",
      header: "Outstanding",
      align: "right",
      render: (i) => formatMoney(subtractAmounts(i.total, i.amount_paid), i.currency),
    },
    {
      key: "status",
      header: "Status",
      render: (i) => (
        <span className="flex flex-wrap items-center gap-1">
          <StatusBadge status={i.status} />
          {isOverdue(i, today) ? <Badge tone="danger">Overdue</Badge> : null}
        </span>
      ),
    },
  ];

  return (
    <div>
      <PageHeader
        title="Invoices"
        description="Customer billing from draft through to payment."
        actions={has("invoices:draft") ? <Button onClick={() => setCreating(true)}>New invoice</Button> : null}
      />

      <FilterBar>
        <SearchInput value={q} onChange={setQ} placeholder="Search invoice number" />
        <Select
          aria-label="Status"
          options={invoiceStatusOptions}
          placeholder="Any status"
          value={status}
          onChange={(e) => setStatus(e.target.value)}
          className="w-full sm:w-48"
        />
        <div className="w-full sm:w-64">
          <CustomerPicker label="" value={customer} onChange={setCustomer} />
        </div>
        <Checkbox label="Overdue only" checked={overdue} onChange={(e) => setOverdue(e.target.checked)} className="h-10" />
      </FilterBar>

      {totals.length > 0 ? (
        <div className="mb-4 grid grid-cols-1 gap-3 sm:grid-cols-3">
          {totals.map((t) => (
            <Card key={t.currency}>
              <CardBody className="py-3">
                <p className="text-xs font-medium uppercase tracking-wide text-slate-500">
                  {t.currency}, {formatNumber(t.count)} on this page
                </p>
                <p className="mt-1 text-lg font-semibold tabular-nums text-slate-900">{formatMoney(t.total, t.currency)}</p>
                <p className="mt-0.5 text-xs text-slate-500">
                  {formatMoney(t.paid, t.currency)} paid, {formatMoney(t.outstanding, t.currency)} outstanding
                </p>
              </CardBody>
            </Card>
          ))}
        </div>
      ) : null}

      <DataTable
        columns={columns}
        rows={list.items}
        rowKey={(i) => i.id}
        loading={list.loading}
        error={list.error}
        onRowClick={(i) => router.push(`/finance/invoices/${i.id}`)}
        pagination={{ page: list.page, perPage: list.perPage, total: list.total, onPage: list.setPage }}
        empty={<EmptyState title="No invoices match" description="Try clearing a filter." />}
      />
      <p className="mt-2 text-xs text-slate-500">
        Totals cover the {list.items.length} {list.items.length === 1 ? "invoice" : "invoices"} on this page, out of{" "}
        {formatNumber(list.total)} matching.
      </p>

      {creating ? (
        <NewInvoiceModal
          onClose={() => setCreating(false)}
          onCreated={(inv) => {
            setCreating(false);
            router.push(`/finance/invoices/${inv.id}`);
          }}
        />
      ) : null}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Draft a new invoice
// ---------------------------------------------------------------------------

interface DraftLine {
  description: string;
  quantity: string;
  unit_price: string;
  tax_rate: string;
}

const EMPTY_LINE: DraftLine = { description: "", quantity: "1", unit_price: "", tax_rate: "0" };

function lineAmount(line: DraftLine): string {
  const qty = Number(line.quantity);
  const price = Number(line.unit_price);
  if (!Number.isFinite(qty) || !Number.isFinite(price)) return "0.00";
  return (Math.round(qty * price * 100) / 100).toFixed(2);
}

function NewInvoiceModal({ onClose, onCreated }: { onClose: () => void; onCreated: (i: Invoice) => void }) {
  const [customer, setCustomer] = useState<CustomerOption | null>(null);
  const [currency, setCurrency] = useState("USD");
  const [dueDays, setDueDays] = useState("30");
  const [notes, setNotes] = useState("");
  const [lines, setLines] = useState<DraftLine[]>([{ ...EMPTY_LINE }]);

  const action = useAction(
    () =>
      api.post<Invoice>("finance/invoices", {
        customer_id: customer?.id,
        currency,
        due_days: Number(dueDays) || 30,
        notes: notes || undefined,
        lines: lines.map((l) => ({
          description: l.description,
          quantity: l.quantity || "1",
          unit_price: l.unit_price || "0",
          tax_rate: l.tax_rate || "0",
        })),
      }),
    { successMessage: "Invoice drafted", onSuccess: onCreated },
  );

  function setLine(index: number, patch: Partial<DraftLine>) {
    setLines((current) => current.map((l, i) => (i === index ? { ...l, ...patch } : l)));
  }

  const subtotal = lines.reduce<string>((acc, l) => addAmounts(acc, lineAmount(l)), "0.00");
  const ready = Boolean(customer) && lines.every((l) => l.description.trim() && l.unit_price !== "");
  const fe = action.fieldErrors;

  return (
    <Modal
      open
      onClose={onClose}
      title="New invoice"
      description="Creates a draft. Totals of 50,000 or more need approval before they can be issued."
      size="xl"
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" form="new-invoice" loading={action.pending} disabled={!ready}>
            Create draft
          </Button>
        </>
      }
    >
      <form
        id="new-invoice"
        className="space-y-4"
        onSubmit={(e: FormEvent) => {
          e.preventDefault();
          if (ready) void action.run();
        }}
      >
        <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
          <CustomerPicker value={customer} onChange={setCustomer} error={fe.customer_id} required />
          <Input label="Currency" maxLength={3} value={currency} onChange={(e) => setCurrency(e.target.value.toUpperCase())} error={fe.currency} />
          <Input label="Payment terms, days" inputMode="numeric" value={dueDays} onChange={(e) => setDueDays(e.target.value)} error={fe.due_days} />
        </div>

        <div>
          <p className="mb-2 text-sm font-medium text-slate-700">Lines</p>
          <div className="space-y-2">
            {lines.map((line, i) => (
              <div key={i} className="grid grid-cols-1 items-end gap-2 rounded-md border border-slate-200 p-2 sm:grid-cols-12">
                <div className="sm:col-span-5">
                  <Input
                    label={i === 0 ? "Description" : undefined}
                    aria-label="Description"
                    value={line.description}
                    onChange={(e) => setLine(i, { description: e.target.value })}
                  />
                </div>
                <div className="sm:col-span-2">
                  <Input
                    label={i === 0 ? "Quantity" : undefined}
                    aria-label="Quantity"
                    inputMode="decimal"
                    value={line.quantity}
                    onChange={(e) => setLine(i, { quantity: e.target.value })}
                  />
                </div>
                <div className="sm:col-span-2">
                  <Input
                    label={i === 0 ? "Unit price" : undefined}
                    aria-label="Unit price"
                    inputMode="decimal"
                    value={line.unit_price}
                    onChange={(e) => setLine(i, { unit_price: e.target.value })}
                  />
                </div>
                <div className="sm:col-span-2">
                  <Input
                    label={i === 0 ? "Tax rate" : undefined}
                    aria-label="Tax rate"
                    inputMode="decimal"
                    value={line.tax_rate}
                    onChange={(e) => setLine(i, { tax_rate: e.target.value })}
                  />
                </div>
                <div className="flex items-center justify-between gap-2 sm:col-span-1">
                  <span className="text-xs tabular-nums text-slate-600 sm:hidden">{lineAmount(line)}</span>
                  <Button
                    variant="ghost"
                    size="sm"
                    aria-label={`Remove line ${i + 1}`}
                    disabled={lines.length === 1}
                    onClick={() => setLines((current) => current.filter((_, index) => index !== i))}
                  >
                    Remove
                  </Button>
                </div>
              </div>
            ))}
          </div>
          <div className="mt-2 flex items-center justify-between">
            <Button variant="secondary" size="sm" onClick={() => setLines((current) => [...current, { ...EMPTY_LINE }])}>
              Add line
            </Button>
            <p className="text-sm text-slate-700">
              Subtotal before tax <span className="font-semibold tabular-nums">{formatMoney(subtotal, currency)}</span>
            </p>
          </div>
        </div>

        <Textarea label="Notes" rows={2} value={notes} onChange={(e) => setNotes(e.target.value)} error={fe.notes} />
      </form>
    </Modal>
  );
}
