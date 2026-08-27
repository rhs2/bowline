"use client";

import { Suspense, useState } from "react";
import { useSearchParams } from "next/navigation";
import { useQuery } from "@/lib/hooks";
import { proxyUrl } from "@/lib/api";
import { formatDate, formatMoney, humanize, MONTH_NAMES, periodLabel, todayIso } from "@/lib/format";
import { toCents } from "@/lib/ledger";
import type { AgingBucket, ArAgingBucketTotal, ArAgingReport, PnlReport, TrialBalanceReport } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { Card, CardBody, CardHeader } from "@/components/ui/Card";
import { Tabs } from "@/components/ui/Tabs";
import { Select } from "@/components/ui/Field";
import { Badge } from "@/components/ui/Badge";
import { EmptyState, ErrorState } from "@/components/ui/States";
import { CardSkeleton, PageSkeleton } from "@/components/ui/Skeleton";

type Tab = "trial-balance" | "aging" | "pnl";

const BUCKET_ORDER: AgingBucket[] = ["current", "1-30", "31-60", "61-90", "90+"];

const BUCKET_LABELS: Record<AgingBucket, string> = {
  current: "Not yet due",
  "1-30": "1 to 30 days",
  "31-60": "31 to 60 days",
  "61-90": "61 to 90 days",
  "90+": "Over 90 days",
};

const BUCKET_TONES: Record<AgingBucket, "neutral" | "info" | "warning" | "danger"> = {
  current: "neutral",
  "1-30": "info",
  "31-60": "warning",
  "61-90": "warning",
  "90+": "danger",
};

export default function ReportsPage() {
  return (
    <Suspense fallback={<PageSkeleton />}>
      <Reports />
    </Suspense>
  );
}

function Reports() {
  const params = useSearchParams();
  const initial = params.get("tab");
  const [tab, setTab] = useState<Tab>(initial === "aging" || initial === "pnl" ? initial : "trial-balance");

  return (
    <div>
      <PageHeader title="Financial reports" description="Read straight from the ledger views. Nothing here is cached." />
      <div className="mb-4">
        <Tabs<Tab>
          tabs={[
            { key: "trial-balance", label: "Trial balance" },
            { key: "aging", label: "AR aging" },
            { key: "pnl", label: "Profit and loss" },
          ]}
          value={tab}
          onChange={setTab}
        />
      </div>
      {tab === "trial-balance" ? <TrialBalance /> : null}
      {tab === "aging" ? <ArAging /> : null}
      {tab === "pnl" ? <Pnl /> : null}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Trial balance
// ---------------------------------------------------------------------------

function TrialBalance() {
  const report = useQuery<TrialBalanceReport>("finance/reports/trial-balance");
  const data = report.data;

  if (report.loading && !data) return <CardSkeleton lines={8} />;
  if (report.error) return <ErrorState error={report.error} onRetry={report.reload} />;
  if (!data) return null;

  // The server reports this; only fall back to comparing the totals if an older
  // build of the API leaves the field out.
  const balanced = data.balanced ?? toCents(data.total_debit) === toCents(data.total_credit);

  return (
    <Card>
      <CardHeader
        title="Trial balance"
        description="Every account with a balance, debits against credits."
        actions={balanced ? <Badge tone="success">Balanced</Badge> : <Badge tone="danger">Out of balance</Badge>}
      />
      <CardBody className="px-0 py-0">
        <div className="overflow-x-auto">
          <table className="min-w-full divide-y divide-slate-200 text-sm">
            <thead className="bg-slate-50">
              <tr>
                <th scope="col" className="px-4 py-2 text-left text-xs font-semibold uppercase tracking-wide text-slate-500">
                  Code
                </th>
                <th scope="col" className="px-4 py-2 text-left text-xs font-semibold uppercase tracking-wide text-slate-500">
                  Account
                </th>
                <th scope="col" className="px-4 py-2 text-left text-xs font-semibold uppercase tracking-wide text-slate-500">
                  Type
                </th>
                <th scope="col" className="px-4 py-2 text-right text-xs font-semibold uppercase tracking-wide text-slate-500">
                  Debit
                </th>
                <th scope="col" className="px-4 py-2 text-right text-xs font-semibold uppercase tracking-wide text-slate-500">
                  Credit
                </th>
                <th scope="col" className="px-4 py-2 text-right text-xs font-semibold uppercase tracking-wide text-slate-500">
                  Balance
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100">
              {data.rows.map((row) => (
                <tr key={row.code}>
                  <td className="px-4 py-2 font-mono text-xs text-slate-700">{row.code}</td>
                  <td className="px-4 py-2 text-slate-900">{row.name}</td>
                  <td className="px-4 py-2 text-slate-600">{humanize(row.type)}</td>
                  <td className="px-4 py-2 text-right tabular-nums">{toCents(row.debit) ? formatMoney(row.debit) : ""}</td>
                  <td className="px-4 py-2 text-right tabular-nums">{toCents(row.credit) ? formatMoney(row.credit) : ""}</td>
                  <td className="px-4 py-2 text-right font-medium tabular-nums text-slate-900">{formatMoney(row.balance)}</td>
                </tr>
              ))}
            </tbody>
            <tfoot className="bg-slate-50 font-semibold">
              <tr>
                <td className="px-4 py-2 text-slate-700" colSpan={3}>
                  Totals
                </td>
                <td className="px-4 py-2 text-right tabular-nums">{formatMoney(data.total_debit)}</td>
                <td className="px-4 py-2 text-right tabular-nums">{formatMoney(data.total_credit)}</td>
                <td className="px-4 py-2" />
              </tr>
            </tfoot>
          </table>
        </div>
        {data.rows.length === 0 ? (
          <div className="p-8">
            <EmptyState title="No balances" description="Nothing has been posted to the ledger yet." />
          </div>
        ) : null}
      </CardBody>
    </Card>
  );
}

// ---------------------------------------------------------------------------
// AR aging
// ---------------------------------------------------------------------------

function ArAging() {
  const report = useQuery<ArAgingReport>("finance/reports/ar-aging");
  const data = report.data;

  if (report.loading && !data) return <CardSkeleton lines={8} />;
  if (report.error) return <ErrorState error={report.error} onRetry={report.reload} />;
  if (!data) return null;

  // The report carries one rollup per bucket that actually has invoices, so index
  // them by name and treat a missing bucket as an empty one.
  const byBucket = new Map<AgingBucket, ArAgingBucketTotal>(
    (data.buckets ?? []).map((b) => [b.bucket, b]),
  );

  return (
    <div className="space-y-4">
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-3 lg:grid-cols-5">
        {BUCKET_ORDER.map((bucket) => (
          <Card key={bucket}>
            <CardBody className="py-3">
              <div className="flex items-center justify-between gap-2">
                <p className="text-xs font-medium uppercase tracking-wide text-slate-500">{BUCKET_LABELS[bucket]}</p>
                <Badge tone={BUCKET_TONES[bucket]}>{byBucket.get(bucket)?.invoices ?? 0}</Badge>
              </div>
              <p className="mt-1 text-lg font-semibold tabular-nums text-slate-900">
                {formatMoney(byBucket.get(bucket)?.outstanding ?? "0")}
              </p>
            </CardBody>
          </Card>
        ))}
      </div>

      <Card>
        <CardHeader
          title="Accounts receivable aging"
          description={`As of ${formatDate(data.as_of)}. Total outstanding ${formatMoney(data.total_outstanding)}, of which ${formatMoney(data.total_overdue)} is overdue.`}
          actions={
            <a
              href={proxyUrl("finance/reports/ar-aging", { format: "xlsx", as_of: data.as_of })}
              className="inline-flex h-8 items-center rounded-md border border-slate-300 bg-white px-3 text-xs font-medium text-slate-800 hover:bg-slate-50"
            >
              Download xlsx
            </a>
          }
        />
        <CardBody className="px-0 py-0">
          <div className="overflow-x-auto">
            <table className="min-w-full divide-y divide-slate-200 text-sm">
              <thead className="bg-slate-50">
                <tr>
                  <th scope="col" className="px-4 py-2 text-left text-xs font-semibold uppercase tracking-wide text-slate-500">
                    Invoice
                  </th>
                  <th scope="col" className="px-4 py-2 text-left text-xs font-semibold uppercase tracking-wide text-slate-500">
                    Customer
                  </th>
                  <th scope="col" className="px-4 py-2 text-left text-xs font-semibold uppercase tracking-wide text-slate-500">
                    Due
                  </th>
                  <th scope="col" className="px-4 py-2 text-right text-xs font-semibold uppercase tracking-wide text-slate-500">
                    Days overdue
                  </th>
                  <th scope="col" className="px-4 py-2 text-left text-xs font-semibold uppercase tracking-wide text-slate-500">
                    Bucket
                  </th>
                  <th scope="col" className="px-4 py-2 text-right text-xs font-semibold uppercase tracking-wide text-slate-500">
                    Outstanding
                  </th>
                </tr>
              </thead>
              <tbody className="divide-y divide-slate-100">
                {data.rows.map((row) => (
                  <tr key={row.invoice_id}>
                    <td className="px-4 py-2">
                      <a href={`/finance/invoices/${row.invoice_id}`} className="font-mono text-xs font-semibold text-slate-900 hover:text-accent-700">
                        {row.invoice_no}
                      </a>
                    </td>
                    <td className="px-4 py-2 text-slate-900">{row.customer_name}</td>
                    <td className="px-4 py-2 text-slate-600">{formatDate(row.due_date)}</td>
                    <td className="px-4 py-2 text-right tabular-nums text-slate-700">
                      {row.days_overdue > 0 ? row.days_overdue : ""}
                    </td>
                    <td className="px-4 py-2">
                      <Badge tone={BUCKET_TONES[row.bucket]}>{BUCKET_LABELS[row.bucket]}</Badge>
                    </td>
                    <td className="px-4 py-2 text-right font-medium tabular-nums text-slate-900">
                      {formatMoney(row.outstanding)}
                    </td>
                  </tr>
                ))}
              </tbody>
              <tfoot className="bg-slate-50 font-semibold">
                <tr>
                  <td className="px-4 py-2 text-slate-700" colSpan={5}>
                    Total outstanding
                  </td>
                  <td className="px-4 py-2 text-right tabular-nums">{formatMoney(data.total_outstanding)}</td>
                </tr>
              </tfoot>
            </table>
          </div>
          {data.rows.length === 0 ? (
            <div className="p-8">
              <EmptyState title="Nothing outstanding" description="Every issued invoice has been paid." />
            </div>
          ) : null}
        </CardBody>
      </Card>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Profit and loss
// ---------------------------------------------------------------------------

function Pnl() {
  const thisYear = Number(todayIso().slice(0, 4));
  const [year, setYear] = useState(String(thisYear));
  const [month, setMonth] = useState("");

  const report = useQuery<PnlReport>("finance/reports/pnl", {
    query: { year, month: month || undefined },
  });
  const data = report.data;

  const yearOptions = [thisYear, thisYear - 1, thisYear - 2].map((y) => ({ value: String(y), label: String(y) }));
  const monthOptions = MONTH_NAMES.map((name, i) => ({ value: String(i + 1), label: name }));

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-end gap-3">
        <Select aria-label="Year" options={yearOptions} value={year} onChange={(e) => setYear(e.target.value)} className="w-32" />
        <Select
          aria-label="Month"
          options={monthOptions}
          placeholder="Whole year"
          value={month}
          onChange={(e) => setMonth(e.target.value)}
          className="w-44"
        />
      </div>

      {report.loading && !data ? (
        <CardSkeleton lines={8} />
      ) : report.error ? (
        <ErrorState error={report.error} onRetry={report.reload} />
      ) : data ? (
        <Card>
          <CardHeader
            title={data.month ? periodLabel(data.year, data.month) : `Year ${data.year}`}
            description="Revenue less expenses, straight from the ledger view."
            actions={
              <Badge tone={toCents(data.net_income) >= 0 ? "success" : "danger"}>
                Net {formatMoney(data.net_income)}
              </Badge>
            }
          />
          <CardBody className="grid grid-cols-1 gap-6 md:grid-cols-2">
            <PnlColumn title="Revenue" lines={data.revenue} total={data.total_revenue} />
            <PnlColumn title="Expenses" lines={data.expenses} total={data.total_expenses} />
          </CardBody>
        </Card>
      ) : null}
    </div>
  );
}

function PnlColumn({
  title,
  lines,
  total,
}: {
  title: string;
  lines: PnlReport["revenue"];
  total: string;
}) {
  return (
    <section>
      <h3 className="mb-2 text-sm font-semibold uppercase tracking-wide text-slate-500">{title}</h3>
      {lines.length === 0 ? (
        <p className="text-sm text-slate-500">Nothing posted in this period.</p>
      ) : (
        <table className="min-w-full text-sm">
          <tbody className="divide-y divide-slate-100">
            {lines.map((line) => (
              <tr key={line.code}>
                <td className="py-2 pr-2 font-mono text-xs text-slate-600">{line.code}</td>
                <td className="py-2 text-slate-900">{line.name}</td>
                <td className="py-2 text-right tabular-nums text-slate-900">{formatMoney(line.amount)}</td>
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr className="border-t border-slate-300">
              <td className="py-2 font-semibold text-slate-900" colSpan={2}>
                Total {title.toLowerCase()}
              </td>
              <td className="py-2 text-right font-semibold tabular-nums text-slate-900">{formatMoney(total)}</td>
            </tr>
          </tfoot>
        </table>
      )}
    </section>
  );
}
