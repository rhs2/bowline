"use client";

import { Suspense, useMemo, useState, type FormEvent } from "react";
import { useSearchParams } from "next/navigation";
import { useMe } from "@/lib/me";
import { useList, useQuery } from "@/lib/hooks";
import { api, asItems } from "@/lib/api";
import { useAction } from "@/lib/forms";
import { formatDate, formatDateTime, formatMoney, humanize, periodLabel, todayIso } from "@/lib/format";
import { fromCents, toCents, validateEntryLines, type LedgerTotals } from "@/lib/ledger";
import type { Account, FiscalPeriod, JournalEntry, ListEnvelope } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { Card, CardBody, CardHeader } from "@/components/ui/Card";
import { FilterBar } from "@/components/ui/Filters";
import { Button } from "@/components/ui/Button";
import { FormError, Input, Select } from "@/components/ui/Field";
import { Modal } from "@/components/ui/Modal";
import { Badge } from "@/components/ui/Badge";
import { EmptyState, ErrorState } from "@/components/ui/States";
import { CardSkeleton, PageSkeleton } from "@/components/ui/Skeleton";

export default function LedgerPage() {
  return (
    <Suspense fallback={<PageSkeleton />}>
      <Ledger />
    </Suspense>
  );
}

function Ledger() {
  const params = useSearchParams();
  const { has } = useMe();
  const [periodId, setPeriodId] = useState("");
  const [accountCode, setAccountCode] = useState("");
  const [posting, setPosting] = useState(params.get("tab") === "new");

  const accounts = useQuery<ListEnvelope<Account> | Account[]>("finance/accounts");
  const periods = useQuery<ListEnvelope<FiscalPeriod> | FiscalPeriod[]>("finance/periods");
  const accountRows = asItems(accounts.data);
  const periodRows = [...asItems(periods.data)].sort((a, b) => b.year - a.year || b.month - a.month);

  const filters = useMemo(() => ({ period_id: periodId, account: accountCode }), [periodId, accountCode]);
  const list = useList<JournalEntry>("finance/journal", filters, { perPage: 20 });

  const canPost = has("ledger:post");

  return (
    <div>
      <PageHeader
        title="General ledger"
        description="Every journal entry, with the lines that make it balance. Entries are immutable; corrections are posted as reversals."
        actions={canPost ? <Button onClick={() => setPosting(true)}>New entry</Button> : null}
      />

      <FilterBar>
        <Select
          aria-label="Period"
          options={periodRows.map((p) => ({
            value: p.id,
            label: `${periodLabel(p.year, p.month)}${p.status === "closed" ? ", closed" : ""}`,
          }))}
          placeholder="Any period"
          value={periodId}
          onChange={(e) => setPeriodId(e.target.value)}
          className="w-full sm:w-56"
        />
        <Select
          aria-label="Account"
          options={accountRows.map((a) => ({ value: a.code, label: `${a.code} ${a.name}` }))}
          placeholder="Any account"
          value={accountCode}
          onChange={(e) => setAccountCode(e.target.value)}
          className="w-full sm:w-72"
        />
      </FilterBar>

      {list.error ? (
        <ErrorState error={list.error} onRetry={list.reload} />
      ) : list.loading && list.items.length === 0 ? (
        <div className="space-y-3">
          <CardSkeleton lines={3} />
          <CardSkeleton lines={3} />
        </div>
      ) : list.items.length === 0 ? (
        <Card>
          <CardBody>
            <EmptyState title="No journal entries" description="Nothing has been posted for this filter." />
          </CardBody>
        </Card>
      ) : (
        <ul className="space-y-3">
          {list.items.map((entry) => (
            <li key={entry.id}>
              <EntryCard entry={entry} canPost={canPost} onReversed={() => list.reload()} />
            </li>
          ))}
        </ul>
      )}

      {list.total > list.perPage ? (
        <div className="mt-4 flex items-center justify-between text-sm">
          <p className="text-slate-600">
            Page {list.page} of {Math.max(1, Math.ceil(list.total / list.perPage))}, {list.total} entries
          </p>
          <div className="flex gap-2">
            <Button variant="secondary" size="sm" disabled={list.page <= 1} onClick={() => list.setPage(list.page - 1)}>
              Previous
            </Button>
            <Button
              variant="secondary"
              size="sm"
              disabled={list.page >= Math.ceil(list.total / list.perPage)}
              onClick={() => list.setPage(list.page + 1)}
            >
              Next
            </Button>
          </div>
        </div>
      ) : null}

      {posting ? (
        <NewEntryModal
          accounts={accountRows}
          onClose={() => setPosting(false)}
          onCreated={() => {
            setPosting(false);
            list.reload();
          }}
        />
      ) : null}
    </div>
  );
}

function EntryCard({
  entry,
  canPost,
  onReversed,
}: {
  entry: JournalEntry;
  canPost: boolean;
  onReversed: () => void;
}) {
  const [confirming, setConfirming] = useState(false);
  const reverse = useAction(() => api.post(`finance/journal/${entry.id}/reverse`), {
    successMessage: "Reversing entry posted",
    onSuccess: () => {
      setConfirming(false);
      onReversed();
    },
  });

  const debit = entry.lines.reduce((acc, l) => acc + toCents(l.debit), 0);
  const credit = entry.lines.reduce((acc, l) => acc + toCents(l.credit), 0);
  const reversed = entry.reversed_by_entry_id !== null;

  return (
    <Card>
      <CardHeader
        title={
          <span className="flex flex-wrap items-center gap-2">
            <span className="font-mono text-sm">Entry {entry.entry_no}</span>
            <Badge tone="neutral">{humanize(entry.source_type)}</Badge>
            {reversed ? <Badge tone="danger">Reversed</Badge> : null}
            {entry.reverses_entry_id ? <Badge tone="warning">Reversal</Badge> : null}
          </span>
        }
        description={`${formatDate(entry.entry_date)}, ${entry.memo}`}
        actions={
          canPost && !reversed && !entry.reverses_entry_id ? (
            <Button variant="secondary" size="sm" onClick={() => setConfirming(true)}>
              Reverse
            </Button>
          ) : null
        }
      />
      <CardBody className="px-0 py-0">
        <div className="overflow-x-auto">
          <table className="min-w-full divide-y divide-slate-200 text-sm">
            <thead className="bg-slate-50">
              <tr>
                <th scope="col" className="px-4 py-2 text-left text-xs font-semibold uppercase tracking-wide text-slate-500">
                  Account
                </th>
                <th scope="col" className="px-4 py-2 text-left text-xs font-semibold uppercase tracking-wide text-slate-500">
                  Description
                </th>
                <th scope="col" className="px-4 py-2 text-right text-xs font-semibold uppercase tracking-wide text-slate-500">
                  Debit
                </th>
                <th scope="col" className="px-4 py-2 text-right text-xs font-semibold uppercase tracking-wide text-slate-500">
                  Credit
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100">
              {entry.lines.map((line) => (
                <tr key={line.id}>
                  <td className="px-4 py-2 text-slate-800">
                    <span className="font-mono text-xs">{line.account_code}</span>
                    {line.account_name ? <span className="ml-2 text-slate-600">{line.account_name}</span> : null}
                  </td>
                  <td className="px-4 py-2 text-slate-600">{line.description ?? ""}</td>
                  <td className="px-4 py-2 text-right tabular-nums text-slate-800">
                    {toCents(line.debit) > 0 ? formatMoney(line.debit) : ""}
                  </td>
                  <td className="px-4 py-2 text-right tabular-nums text-slate-800">
                    {toCents(line.credit) > 0 ? formatMoney(line.credit) : ""}
                  </td>
                </tr>
              ))}
            </tbody>
            <tfoot className="bg-slate-50 font-medium">
              <tr>
                <td className="px-4 py-2 text-slate-600" colSpan={2}>
                  Totals
                </td>
                <td className="px-4 py-2 text-right tabular-nums">{formatMoney(fromCents(debit))}</td>
                <td className="px-4 py-2 text-right tabular-nums">{formatMoney(fromCents(credit))}</td>
              </tr>
            </tfoot>
          </table>
        </div>
        <p className="px-4 py-2 text-xs text-slate-500">
          Posted {formatDateTime(entry.posted_at)}
          {entry.posted_by_name ? ` by ${entry.posted_by_name}` : ""}.
        </p>
      </CardBody>

      {confirming ? (
        <Modal
          open
          onClose={() => setConfirming(false)}
          title={`Reverse entry ${entry.entry_no}`}
          description="Posts the mirror entry and links the two. The original stays in the ledger."
          footer={
            <>
              <Button variant="secondary" onClick={() => setConfirming(false)}>
                Cancel
              </Button>
              <Button variant="danger" loading={reverse.pending} onClick={() => void reverse.run()}>
                Post reversal
              </Button>
            </>
          }
        >
          <FormError message={reverse.error?.message} />
          <p className="text-sm text-slate-600">{entry.memo}</p>
        </Modal>
      ) : null}
    </Card>
  );
}

// ---------------------------------------------------------------------------
// Manual entry
// ---------------------------------------------------------------------------

interface EntryLine {
  account_code: string;
  debit: string;
  credit: string;
  description: string;
}

const BLANK_LINE: EntryLine = { account_code: "", debit: "", credit: "", description: "" };

function NewEntryModal({
  accounts,
  onClose,
  onCreated,
}: {
  accounts: Account[];
  onClose: () => void;
  onCreated: () => void;
}) {
  const [entryDate, setEntryDate] = useState(todayIso());
  const [memo, setMemo] = useState("");
  const [lines, setLines] = useState<EntryLine[]>([{ ...BLANK_LINE }, { ...BLANK_LINE }]);

  const action = useAction(
    () =>
      api.post<JournalEntry>("finance/journal", {
        entry_date: entryDate,
        memo,
        lines: lines.map((l) => ({
          account_code: l.account_code,
          debit: l.debit || "0",
          credit: l.credit || "0",
          description: l.description,
        })),
      }),
    { successMessage: "Journal entry posted", onSuccess: onCreated },
  );

  function setLine(index: number, patch: Partial<EntryLine>) {
    setLines((current) => current.map((l, i) => (i === index ? { ...l, ...patch } : l)));
  }

  const validation = validateEntryLines(lines);
  const totals: LedgerTotals = validation.totals;
  const canSubmit = validation.ok && memo.trim().length > 0;
  const accountOptions = accounts
    .filter((a) => a.active)
    .map((a) => ({ value: a.code, label: `${a.code} ${a.name}` }));
  const generalProblems = validation.problems.filter((p) => p.index === -1);

  return (
    <Modal
      open
      onClose={onClose}
      title="New journal entry"
      description="Every line is a debit or a credit. The entry must balance before it can be posted, and posting into a closed period is refused."
      size="xl"
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" form="new-entry" loading={action.pending} disabled={!canSubmit}>
            Post entry
          </Button>
        </>
      }
    >
      <form
        id="new-entry"
        className="space-y-4"
        onSubmit={(e: FormEvent) => {
          e.preventDefault();
          if (canSubmit) void action.run();
        }}
      >
        <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
          <Input label="Entry date" type="date" value={entryDate} onChange={(e) => setEntryDate(e.target.value)} error={action.fieldErrors.entry_date} required />
          <div className="sm:col-span-2">
            <Input label="Memo" value={memo} onChange={(e) => setMemo(e.target.value)} error={action.fieldErrors.memo} required />
          </div>
        </div>

        <div className="space-y-2">
          {lines.map((line, i) => {
            const problem = validation.problems.find((p) => p.index === i);
            return (
              <div key={i} className="grid grid-cols-1 items-end gap-2 rounded-md border border-slate-200 p-2 sm:grid-cols-12">
                <div className="sm:col-span-4">
                  <Select
                    label={i === 0 ? "Account" : undefined}
                    aria-label="Account"
                    options={accountOptions}
                    placeholder="Choose an account"
                    value={line.account_code}
                    onChange={(e) => setLine(i, { account_code: e.target.value })}
                  />
                </div>
                <div className="sm:col-span-3">
                  <Input
                    label={i === 0 ? "Description" : undefined}
                    aria-label="Line description"
                    value={line.description}
                    onChange={(e) => setLine(i, { description: e.target.value })}
                  />
                </div>
                <div className="sm:col-span-2">
                  <Input
                    label={i === 0 ? "Debit" : undefined}
                    aria-label="Debit"
                    inputMode="decimal"
                    value={line.debit}
                    onChange={(e) => setLine(i, { debit: e.target.value, credit: e.target.value ? "" : line.credit })}
                  />
                </div>
                <div className="sm:col-span-2">
                  <Input
                    label={i === 0 ? "Credit" : undefined}
                    aria-label="Credit"
                    inputMode="decimal"
                    value={line.credit}
                    onChange={(e) => setLine(i, { credit: e.target.value, debit: e.target.value ? "" : line.debit })}
                  />
                </div>
                <div className="sm:col-span-1">
                  <Button
                    variant="ghost"
                    size="sm"
                    aria-label={`Remove line ${i + 1}`}
                    disabled={lines.length <= 2}
                    onClick={() => setLines((current) => current.filter((_, index) => index !== i))}
                  >
                    Remove
                  </Button>
                </div>
                {problem ? (
                  <p className="text-xs text-red-600 sm:col-span-12" role="alert">
                    {problem.message}
                  </p>
                ) : null}
              </div>
            );
          })}
        </div>

        <div className="flex flex-wrap items-center justify-between gap-3">
          <Button variant="secondary" size="sm" onClick={() => setLines((current) => [...current, { ...BLANK_LINE }])}>
            Add line
          </Button>
          <div className="flex items-center gap-4 text-sm">
            <span className="text-slate-600">
              Debits <span className="font-semibold tabular-nums text-slate-900">{formatMoney(totals.debit)}</span>
            </span>
            <span className="text-slate-600">
              Credits <span className="font-semibold tabular-nums text-slate-900">{formatMoney(totals.credit)}</span>
            </span>
            {totals.balanced ? (
              <Badge tone="success">Balanced</Badge>
            ) : (
              <Badge tone="warning">Out by {formatMoney(totals.difference)}</Badge>
            )}
          </div>
        </div>

        {generalProblems.length > 0 ? (
          <ul className="list-inside list-disc rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-900">
            {generalProblems.map((p) => (
              <li key={p.message}>{p.message}</li>
            ))}
          </ul>
        ) : null}
      </form>
    </Modal>
  );
}
