"use client";

import { useState, type FormEvent } from "react";
import Link from "next/link";
import { useParams } from "next/navigation";
import { useMe } from "@/lib/me";
import { has } from "@/lib/permissions";
import { useQuery } from "@/lib/hooks";
import { api } from "@/lib/api";
import { useAction } from "@/lib/forms";
import { formatDate, formatDateTime, formatMoney, formatNumber, humanize, todayIso } from "@/lib/format";
import { subtractAmounts, toCents } from "@/lib/ledger";
import { paymentMethodOptions } from "@/lib/options";
import { invoiceActions, type InvoiceAction } from "@/lib/transitions";
import type { Customer, InvoiceDetail, InvoiceLine, Payment } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { Card, CardBody, CardHeader, DescriptionList } from "@/components/ui/Card";
import { DataTable, type Column } from "@/components/ui/DataTable";
import { Button } from "@/components/ui/Button";
import { FormError, Input, Select } from "@/components/ui/Field";
import { Modal } from "@/components/ui/Modal";
import { StatusBadge } from "@/components/StatusBadge";
import { Badge } from "@/components/ui/Badge";
import { PageSkeleton } from "@/components/ui/Skeleton";
import { EmptyState, ErrorState } from "@/components/ui/States";
import { DownloadButton } from "@/components/DownloadButton";

const ACTION_LABELS: Record<InvoiceAction, string> = {
  submit: "Submit for approval",
  approve: "Approve",
  issue: "Issue",
  void: "Void",
  record_payment: "Record payment",
};

export default function InvoicePage() {
  const { id } = useParams<{ id: string }>();
  const { permissions } = useMe();
  const detail = useQuery<InvoiceDetail>(`finance/invoices/${id}`);
  const [confirming, setConfirming] = useState<Exclude<InvoiceAction, "record_payment"> | null>(null);
  const [paying, setPaying] = useState(false);

  const inv = detail.data;
  // The invoice already carries the customer's name. This fetch is only for the
  // richer contact block, and reading a customer record needs `customers:read`,
  // which an executive does not hold. Ask only when the caller may actually see it,
  // rather than firing a request that is always refused.
  const mayReadCustomer = has(permissions, "customers:read");
  const customer = useQuery<Customer>(
    inv && mayReadCustomer ? `ops/customers/${inv.customer_id}` : null,
  );

  if (detail.loading && !inv) return <PageSkeleton />;
  if (detail.error) {
    return (
      <div>
        <PageHeader title="Invoice" backHref="/finance/invoices" backLabel="Invoices" />
        <ErrorState error={detail.error} onRetry={detail.reload} />
      </div>
    );
  }
  if (!inv) return null;

  const actions = invoiceActions(inv.status, permissions);
  const outstanding = subtractAmounts(inv.total, inv.amount_paid);
  const overdue =
    inv.due_date !== null &&
    inv.due_date < todayIso() &&
    toCents(outstanding) > 0 &&
    (inv.status === "issued" || inv.status === "partially_paid");
  const hasPdf = Boolean(inv.pdf_s3_key);

  const lineColumns: Column<InvoiceLine>[] = [
    { key: "seq", header: "#", render: (l) => l.seq },
    { key: "description", header: "Description", render: (l) => l.description },
    { key: "qty", header: "Quantity", align: "right", render: (l) => formatNumber(l.quantity, 2) },
    { key: "price", header: "Unit price", align: "right", render: (l) => formatMoney(l.unit_price, inv.currency) },
    { key: "tax", header: "Tax rate", align: "right", render: (l) => `${formatNumber(Number(l.tax_rate) * 100, 2)}%` },
    { key: "amount", header: "Amount", align: "right", render: (l) => formatMoney(l.amount, inv.currency) },
  ];

  const paymentColumns: Column<Payment>[] = [
    { key: "date", header: "Received", render: (p) => formatDate(p.received_on) },
    { key: "amount", header: "Amount", align: "right", render: (p) => formatMoney(p.amount, inv.currency) },
    { key: "method", header: "Method", render: (p) => humanize(p.method) },
    { key: "reference", header: "Reference", render: (p) => p.reference ?? "", hideOnMobile: true },
    { key: "recorded", header: "Recorded", render: (p) => formatDateTime(p.created_at), hideOnMobile: true },
  ];

  return (
    <div>
      <PageHeader
        title={inv.invoice_no}
        description={`${inv.customer_name ?? "Customer"}, ${formatMoney(inv.total, inv.currency)}`}
        backHref="/finance/invoices"
        backLabel="Invoices"
        meta={
          <>
            <StatusBadge status={inv.status} />
            {overdue ? <Badge tone="danger">Overdue</Badge> : null}
          </>
        }
        actions={
          <>
            {hasPdf ? <DownloadButton path={`finance/invoices/${inv.id}/pdf`}>Open PDF</DownloadButton> : null}
            {actions.map((a) =>
              a === "record_payment" ? (
                <Button key={a} onClick={() => setPaying(true)}>
                  {ACTION_LABELS[a]}
                </Button>
              ) : (
                <Button
                  key={a}
                  variant={a === "void" ? "danger" : a === "issue" ? "success" : "primary"}
                  onClick={() => setConfirming(a)}
                >
                  {ACTION_LABELS[a]}
                </Button>
              ),
            )}
          </>
        }
      />

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
        <div className="space-y-4 lg:col-span-2">
          <Card>
            <CardHeader title="Lines" />
            <CardBody className="px-0 py-0">
              <DataTable
                columns={lineColumns}
                rows={[...inv.lines].sort((a, b) => a.seq - b.seq)}
                rowKey={(l) => l.id}
                dense
                empty={<EmptyState title="No lines" description="This invoice has no billable lines yet." />}
                footer={
                  <>
                    <tr>
                      <td className="px-3 py-2 text-right text-slate-600" colSpan={5}>
                        Subtotal
                      </td>
                      <td className="px-3 py-2 text-right tabular-nums">{formatMoney(inv.subtotal, inv.currency)}</td>
                    </tr>
                    <tr>
                      <td className="px-3 py-2 text-right text-slate-600" colSpan={5}>
                        Tax
                      </td>
                      <td className="px-3 py-2 text-right tabular-nums">{formatMoney(inv.tax, inv.currency)}</td>
                    </tr>
                    <tr className="border-t border-slate-200">
                      <td className="px-3 py-2 text-right font-semibold text-slate-900" colSpan={5}>
                        Total
                      </td>
                      <td className="px-3 py-2 text-right font-semibold tabular-nums text-slate-900">
                        {formatMoney(inv.total, inv.currency)}
                      </td>
                    </tr>
                    <tr>
                      <td className="px-3 py-2 text-right text-slate-600" colSpan={5}>
                        Paid
                      </td>
                      <td className="px-3 py-2 text-right tabular-nums">{formatMoney(inv.amount_paid, inv.currency)}</td>
                    </tr>
                    <tr>
                      <td className="px-3 py-2 text-right font-semibold text-slate-900" colSpan={5}>
                        Outstanding
                      </td>
                      <td className="px-3 py-2 text-right font-semibold tabular-nums text-slate-900">
                        {formatMoney(outstanding, inv.currency)}
                      </td>
                    </tr>
                  </>
                }
              />
            </CardBody>
          </Card>

          <Card>
            <CardHeader
              title="Payments"
              description={`${inv.payments.length} recorded`}
              actions={
                actions.includes("record_payment") ? (
                  <Button size="sm" onClick={() => setPaying(true)}>
                    Record payment
                  </Button>
                ) : null
              }
            />
            <CardBody className="px-0 py-0">
              <DataTable
                columns={paymentColumns}
                rows={inv.payments}
                rowKey={(p) => p.id}
                dense
                empty={<EmptyState title="No payments" description="Payments post Cash against Accounts Receivable." />}
              />
            </CardBody>
          </Card>
        </div>

        <div className="space-y-4">
          <Card>
            <CardHeader title="Customer" />
            <CardBody>
              {customer.data ? (
                <DescriptionList
                  columns={1}
                  items={[
                    {
                      label: "Name",
                      value: (
                        <Link href="/ops/customers" className="font-medium text-accent-700 hover:underline">
                          {customer.data.name}
                        </Link>
                      ),
                    },
                    { label: "Code", value: <span className="font-mono">{customer.data.code}</span> },
                    { label: "Contact", value: customer.data.contact_name },
                    { label: "Email", value: customer.data.contact_email },
                    { label: "Phone", value: customer.data.phone },
                    { label: "Credit limit", value: formatMoney(customer.data.credit_limit, customer.data.currency) },
                    {
                      label: "Billing address",
                      value: [
                        customer.data.billing_address?.line1,
                        customer.data.billing_address?.city,
                        customer.data.billing_address?.country,
                      ]
                        .filter(Boolean)
                        .join(", "),
                    },
                  ]}
                />
              ) : customer.error ? (
                <p className="text-sm text-slate-600">{inv.customer_name ?? "Customer details are not available to you."}</p>
              ) : (
                <p className="text-sm text-slate-500">Loading customer</p>
              )}
            </CardBody>
          </Card>

          <Card>
            <CardHeader title="Invoice" />
            <CardBody>
              <DescriptionList
                columns={1}
                items={[
                  { label: "Status", value: <StatusBadge status={inv.status} /> },
                  { label: "Issued", value: inv.issue_date ? formatDate(inv.issue_date) : null },
                  { label: "Due", value: inv.due_date ? formatDate(inv.due_date) : null },
                  { label: "Currency", value: inv.currency },
                  {
                    label: "Shipment",
                    value: inv.shipment_id ? (
                      <Link href={`/ops/shipments/${inv.shipment_id}`} className="font-medium text-accent-700 hover:underline">
                        {inv.shipment_reference ?? "Shipment"}
                      </Link>
                    ) : null,
                  },
                  {
                    label: "Journal entry",
                    value: inv.journal_entry_id ? (
                      <Link href="/finance/ledger" className="font-medium text-accent-700 hover:underline">
                        Posted to the ledger
                      </Link>
                    ) : null,
                  },
                  { label: "Notes", value: inv.notes },
                  { label: "Created", value: formatDateTime(inv.created_at) },
                ]}
              />
            </CardBody>
          </Card>
        </div>
      </div>

      {confirming ? (
        <ConfirmActionModal
          invoiceId={inv.id}
          action={confirming}
          total={formatMoney(inv.total, inv.currency)}
          onClose={() => setConfirming(null)}
          onDone={() => {
            setConfirming(null);
            detail.reload();
          }}
        />
      ) : null}
      {paying ? (
        <RecordPaymentModal
          invoiceId={inv.id}
          currency={inv.currency}
          outstanding={outstanding}
          onClose={() => setPaying(false)}
          onDone={() => {
            setPaying(false);
            detail.reload();
          }}
        />
      ) : null}
    </div>
  );
}

function ConfirmActionModal({
  invoiceId,
  action,
  total,
  onClose,
  onDone,
}: {
  invoiceId: string;
  action: Exclude<InvoiceAction, "record_payment">;
  total: string;
  onClose: () => void;
  onDone: () => void;
}) {
  const done: Record<typeof action, string> = {
    submit: "Invoice sent for approval",
    approve: "Invoice approved",
    issue: "Invoice issued",
    void: "Invoice voided",
  };
  const run = useAction(() => api.post(`finance/invoices/${invoiceId}/${action}`), {
    successMessage: done[action],
    onSuccess: onDone,
  });

  const explanation: Record<typeof action, string> = {
    submit: `Sends the invoice for approval. Totals of 50,000 or more need an approver before they can be issued. This invoice is ${total}.`,
    approve: "Marks the invoice approved so it can be issued.",
    issue: "Posts Accounts Receivable against Revenue, renders the PDF and makes the invoice payable.",
    void: "Voids the invoice. If it was already issued, a reversing journal entry is posted.",
  };

  return (
    <Modal
      open
      onClose={onClose}
      title={ACTION_LABELS[action]}
      description={explanation[action]}
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button variant={action === "void" ? "danger" : "primary"} loading={run.pending} onClick={() => void run.run()}>
            {ACTION_LABELS[action]}
          </Button>
        </>
      }
    >
      <FormError message={run.error?.message} />
      <p className="text-sm text-slate-600">This is recorded in the audit log against your account.</p>
    </Modal>
  );
}

function RecordPaymentModal({
  invoiceId,
  currency,
  outstanding,
  onClose,
  onDone,
}: {
  invoiceId: string;
  currency: string;
  outstanding: string;
  onClose: () => void;
  onDone: () => void;
}) {
  const [amount, setAmount] = useState(outstanding);
  const [receivedOn, setReceivedOn] = useState(todayIso());
  const [method, setMethod] = useState("bank_transfer");
  const [reference, setReference] = useState("");

  const action = useAction(
    () =>
      api.post<Payment>("finance/payments", {
        invoice_id: invoiceId,
        received_on: receivedOn,
        amount,
        method,
        reference: reference || undefined,
      }),
    { successMessage: "Payment recorded", onSuccess: onDone },
  );

  const overpaying = toCents(amount || "0") > toCents(outstanding);
  const positive = toCents(amount || "0") > 0;
  const fe = action.fieldErrors;

  return (
    <Modal
      open
      onClose={onClose}
      title="Record a payment"
      description={`${formatMoney(outstanding, currency)} is outstanding. Overpayment is rejected by the API.`}
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" form="record-payment" loading={action.pending} disabled={overpaying || !positive}>
            Record payment
          </Button>
        </>
      }
    >
      <form
        id="record-payment"
        className="space-y-3"
        onSubmit={(e: FormEvent) => {
          e.preventDefault();
          if (!overpaying && positive) void action.run();
        }}
      >
        <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <Input
            label={`Amount, ${currency}`}
            inputMode="decimal"
            value={amount}
            onChange={(e) => setAmount(e.target.value)}
            error={fe.amount ?? (overpaying ? "That is more than the outstanding balance" : undefined)}
            required
          />
          <Input label="Received on" type="date" value={receivedOn} onChange={(e) => setReceivedOn(e.target.value)} error={fe.received_on} required />
          <Select label="Method" options={paymentMethodOptions} value={method} onChange={(e) => setMethod(e.target.value)} error={fe.method} />
          <Input label="Reference" value={reference} onChange={(e) => setReference(e.target.value)} error={fe.reference} hint="Bank reference or cheque number." />
        </div>
        <p className="text-xs text-slate-500">Recording a payment posts Cash against Accounts Receivable.</p>
      </form>
    </Modal>
  );
}
