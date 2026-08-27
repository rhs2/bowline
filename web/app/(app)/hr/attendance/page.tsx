"use client";

import { useMemo } from "react";
import { useMe } from "@/lib/me";
import { useNow, useQuery } from "@/lib/hooks";
import { api, asItems } from "@/lib/api";
import { useAction } from "@/lib/forms";
import { addDays, formatDate, formatDateTime, formatTime, todayIso } from "@/lib/format";
import type { AttendanceRecord, ListEnvelope, Shift } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { Card, CardBody, CardHeader } from "@/components/ui/Card";
import { DataTable, type Column } from "@/components/ui/DataTable";
import { Button } from "@/components/ui/Button";
import { Badge } from "@/components/ui/Badge";
import { EmptyState } from "@/components/ui/States";
import { Skeleton } from "@/components/ui/Skeleton";

const HISTORY_DAYS = 30;

/** Elapsed time since clock-in, as "7h 12m". */
function elapsed(from: string, now: Date): string {
  const ms = Math.max(0, now.getTime() - new Date(from).getTime());
  const h = Math.floor(ms / 3600000);
  const m = Math.floor((ms % 3600000) / 60000);
  return `${h}h ${m.toString().padStart(2, "0")}m`;
}

function worked(record: AttendanceRecord): string {
  if (!record.clock_out) return "";
  const ms = new Date(record.clock_out).getTime() - new Date(record.clock_in).getTime();
  if (!Number.isFinite(ms) || ms < 0) return "";
  const h = Math.floor(ms / 3600000);
  const m = Math.floor((ms % 3600000) / 60000);
  return `${h}h ${m.toString().padStart(2, "0")}m`;
}

export default function AttendancePage() {
  const { employee } = useMe();
  const now = useNow(30000);
  const today = todayIso();

  const historyRange = useMemo(() => ({ from: addDays(today, -HISTORY_DAYS), to: today }), [today]);

  // Scoped like every other list, so without an employee_id a supervisor would get
  // their whole team's clock-ins here. That matters beyond the table: the open
  // record below decides whether the button says clock in or clock out.
  const history = useQuery<ListEnvelope<AttendanceRecord> | AttendanceRecord[]>(
    employee ? "hr/attendance" : null,
    { query: { employee_id: employee?.id, from: historyRange.from, to: historyRange.to } },
  );
  const todayShifts = useQuery<ListEnvelope<Shift> | Shift[]>(employee ? "hr/shifts" : null, {
    query: { employee_id: employee?.id, from: today, to: today },
  });

  const records = asItems(history.data)
    .slice()
    .sort((a, b) => b.clock_in.localeCompare(a.clock_in));
  const todayRecords = records.filter((r) => r.clock_in.slice(0, 10) === today);
  const openRecord = todayRecords.find((r) => r.clock_out === null) ?? null;
  const lastToday = todayRecords[0] ?? null;
  const shiftToday = asItems(todayShifts.data).sort((a, b) => a.starts_at.localeCompare(b.starts_at))[0] ?? null;

  const clockIn = useAction(() => api.post<AttendanceRecord>("hr/attendance/clock-in", shiftToday ? { shift_id: shiftToday.id } : {}), {
    successMessage: "Clocked in",
    onSuccess: () => history.reload(),
  });
  const clockOut = useAction(() => api.post<AttendanceRecord>("hr/attendance/clock-out", {}), {
    successMessage: "Clocked out",
    onSuccess: () => history.reload(),
  });

  const columns: Column<AttendanceRecord>[] = [
    { key: "date", header: "Date", render: (r) => formatDate(r.clock_in.slice(0, 10)) },
    { key: "in", header: "Clock in", render: (r) => <span className="tabular-nums">{formatTime(r.clock_in)}</span> },
    {
      key: "out",
      header: "Clock out",
      render: (r) =>
        r.clock_out ? (
          <span className="tabular-nums">{formatTime(r.clock_out)}</span>
        ) : (
          <span className="text-slate-400">Still open</span>
        ),
    },
    { key: "worked", header: "Worked", align: "right", render: (r) => worked(r), hideOnMobile: true },
    {
      key: "late",
      header: "On time",
      render: (r) => (r.late ? <Badge tone="warning">Late</Badge> : <Badge tone="success">On time</Badge>),
    },
    { key: "source", header: "Source", render: (r) => r.source, hideOnMobile: true },
  ];

  const lateCount = records.filter((r) => r.late).length;

  return (
    <div>
      <PageHeader
        title="Attendance"
        description="Clock in when your shift starts. A clock-in more than ten minutes after the shift start counts as late."
      />

      <Card className="mb-6">
        <CardHeader
          title="Today"
          description={formatDate(today)}
          actions={shiftToday ? <Badge tone="accent">Shift {formatTime(shiftToday.starts_at)} to {formatTime(shiftToday.ends_at)}, {shiftToday.site}</Badge> : null}
        />
        <CardBody>
          {history.loading && records.length === 0 ? (
            <Skeleton className="h-16 w-full" />
          ) : (
            <div className="flex flex-col items-start gap-4 sm:flex-row sm:items-center sm:justify-between">
              <div>
                {openRecord ? (
                  <>
                    <p className="text-sm text-slate-600">
                      Clocked in at <span className="font-medium text-slate-900">{formatTime(openRecord.clock_in)}</span>
                      {openRecord.late ? <Badge tone="warning" className="ml-2">Late</Badge> : null}
                    </p>
                    <p className="mt-1 text-3xl font-semibold tabular-nums tracking-tight text-slate-900">
                      {elapsed(openRecord.clock_in, now)}
                    </p>
                    <p className="mt-1 text-xs text-slate-500">on the clock so far</p>
                  </>
                ) : lastToday ? (
                  <>
                    <p className="text-sm text-slate-600">You are clocked out.</p>
                    <p className="mt-1 text-sm text-slate-900">
                      Last entry {formatTime(lastToday.clock_in)} to {formatTime(lastToday.clock_out)}, {worked(lastToday)}{" "}
                      worked.
                    </p>
                  </>
                ) : (
                  <>
                    <p className="text-sm text-slate-600">You have not clocked in today.</p>
                    {shiftToday ? (
                      <p className="mt-1 text-sm text-slate-900">
                        Your shift starts at {formatTime(shiftToday.starts_at)} at {shiftToday.site}.
                      </p>
                    ) : (
                      <p className="mt-1 text-sm text-slate-500">No shift is scheduled for you today.</p>
                    )}
                  </>
                )}
              </div>
              <div className="flex w-full gap-3 sm:w-auto">
                <Button
                  size="xl"
                  className="flex-1 sm:flex-none"
                  disabled={openRecord !== null}
                  loading={clockIn.pending}
                  onClick={() => void clockIn.run()}
                >
                  Clock in
                </Button>
                <Button
                  size="xl"
                  variant="secondary"
                  className="flex-1 sm:flex-none"
                  disabled={openRecord === null}
                  loading={clockOut.pending}
                  onClick={() => void clockOut.run()}
                >
                  Clock out
                </Button>
              </div>
            </div>
          )}
        </CardBody>
      </Card>

      <section aria-label="History">
        <div className="mb-2 flex flex-wrap items-baseline justify-between gap-2">
          <h2 className="text-sm font-semibold uppercase tracking-wide text-slate-500">Last thirty days</h2>
          <p className="text-xs text-slate-500">
            {records.length} {records.length === 1 ? "entry" : "entries"}
            {lateCount > 0 ? `, ${lateCount} late` : ""}
          </p>
        </div>
        <DataTable
          columns={columns}
          rows={records}
          rowKey={(r) => r.id}
          loading={history.loading}
          error={history.error}
          dense
          empty={<EmptyState title="No attendance yet" description="Your clock-ins from the last thirty days appear here." />}
        />
        {records.length > 0 ? (
          <p className="mt-2 text-xs text-slate-500">
            Most recent entry {formatDateTime(records[0]?.clock_in)}.
          </p>
        ) : null}
      </section>
    </div>
  );
}
