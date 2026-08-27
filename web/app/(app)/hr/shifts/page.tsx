"use client";

import { useMemo, useState, type FormEvent } from "react";
import { useMe } from "@/lib/me";
import { useQuery } from "@/lib/hooks";
import { api, asItems } from "@/lib/api";
import { useAction } from "@/lib/forms";
import { addDays, formatDate, formatTime, isoToLocalInput, localInputToIso, todayIso } from "@/lib/format";
import type { ListEnvelope, Shift, Site } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { Card, CardBody, CardHeader } from "@/components/ui/Card";
import { DataTable, type Column } from "@/components/ui/DataTable";
import { Button } from "@/components/ui/Button";
import { FormError, Input, Select } from "@/components/ui/Field";
import { Modal } from "@/components/ui/Modal";
import { StatusBadge } from "@/components/StatusBadge";
import { EmptyState } from "@/components/ui/States";
import { EmployeePicker, type EmployeeOption } from "@/components/pickers/EmployeePicker";

const PAST_WINDOW_DAYS = 30;
const FUTURE_WINDOW_DAYS = 60;

export default function ShiftsPage() {
  const { employee, can } = useMe();
  const canSchedule = can("shifts:manage:subtree");
  const [report, setReport] = useState<EmployeeOption | null>(null);
  const [scheduling, setScheduling] = useState(false);

  const today = todayIso();
  const range = useMemo(
    () => ({ from: addDays(today, -PAST_WINDOW_DAYS), to: addDays(today, FUTURE_WINDOW_DAYS) }),
    [today],
  );

  const targetId = report?.id ?? employee?.id ?? null;
  const shifts = useQuery<ListEnvelope<Shift> | Shift[]>(targetId ? "hr/shifts" : null, {
    query: { employee_id: targetId, from: range.from, to: range.to },
  });

  const rows = asItems(shifts.data);
  const now = Date.now();
  const upcoming = rows
    .filter((s) => new Date(s.ends_at).getTime() >= now)
    .sort((a, b) => a.starts_at.localeCompare(b.starts_at));
  const past = rows
    .filter((s) => new Date(s.ends_at).getTime() < now)
    .sort((a, b) => b.starts_at.localeCompare(a.starts_at));

  const columns: Column<Shift>[] = [
    { key: "date", header: "Date", render: (s) => formatDate(s.starts_at.slice(0, 10)) },
    {
      key: "time",
      header: "Time",
      render: (s) => (
        <span className="tabular-nums">
          {formatTime(s.starts_at)} to {formatTime(s.ends_at)}
        </span>
      ),
    },
    { key: "site", header: "Site", render: (s) => s.site },
    { key: "role", header: "Role", render: (s) => s.role_on_shift ?? "", hideOnMobile: true },
    { key: "status", header: "Status", render: (s) => <StatusBadge status={s.status} /> },
  ];

  const viewingSelf = !report || report.id === employee?.id;

  return (
    <div>
      <PageHeader
        title="Shifts"
        description={
          viewingSelf
            ? "Your roster for the next two months and the last thirty days."
            : `Roster for ${report.name}.`
        }
        actions={canSchedule ? <Button onClick={() => setScheduling(true)}>Schedule a shift</Button> : null}
      />

      {canSchedule ? (
        <Card className="mb-4">
          <CardHeader
            title="Whose roster"
            description="Pick anyone who reports up to you, or clear the field to see your own."
          />
          <CardBody>
            <div className="max-w-md">
              <EmployeePicker
                label="Employee"
                value={report}
                onChange={setReport}
                emptyMessage="No matching person reports up to you. Scheduling is limited to your subtree."
              />
            </div>
          </CardBody>
        </Card>
      ) : null}

      <div className="space-y-6">
        <section aria-label="Upcoming shifts">
          <h2 className="mb-2 text-sm font-semibold uppercase tracking-wide text-slate-500">Upcoming</h2>
          <DataTable
            columns={columns}
            rows={upcoming}
            rowKey={(s) => s.id}
            loading={shifts.loading}
            error={shifts.error}
            dense
            empty={<EmptyState title="No upcoming shifts" description="Nothing is scheduled in the next sixty days." />}
          />
        </section>

        <section aria-label="Past shifts">
          <h2 className="mb-2 text-sm font-semibold uppercase tracking-wide text-slate-500">Last thirty days</h2>
          <DataTable
            columns={columns}
            rows={past}
            rowKey={(s) => s.id}
            loading={shifts.loading}
            error={shifts.error}
            dense
            empty={<EmptyState title="No past shifts" description="Nothing was scheduled in the last thirty days." />}
          />
        </section>
      </div>

      {scheduling ? (
        <ScheduleModal
          defaultEmployee={report}
          onClose={() => setScheduling(false)}
          onCreated={(shift) => {
            setScheduling(false);
            if (shift.employee_id === targetId) shifts.reload();
          }}
        />
      ) : null}
    </div>
  );
}

function ScheduleModal({
  defaultEmployee,
  onClose,
  onCreated,
}: {
  defaultEmployee: EmployeeOption | null;
  onClose: () => void;
  onCreated: (shift: Shift) => void;
}) {
  const sites = useQuery<ListEnvelope<Site> | Site[]>("ops/sites");
  const [employee, setEmployee] = useState<EmployeeOption | null>(defaultEmployee);
  const [site, setSite] = useState("");
  const [startsAt, setStartsAt] = useState(() => defaultStart());
  const [endsAt, setEndsAt] = useState(() => defaultEnd());
  const [role, setRole] = useState("");

  const action = useAction(
    () =>
      api.post<Shift>("hr/shifts", {
        employee_id: employee?.id,
        site,
        starts_at: localInputToIso(startsAt),
        ends_at: localInputToIso(endsAt),
        role_on_shift: role || null,
      }),
    { successMessage: "Shift scheduled", onSuccess: onCreated },
  );

  const siteNames = asItems(sites.data).map((s) => s.name);
  const ordered = localInputToIso(endsAt) > localInputToIso(startsAt);
  const fe = action.fieldErrors;

  return (
    <Modal
      open
      onClose={onClose}
      title="Schedule a shift"
      description="Shifts can only be scheduled for people who report up to you."
      size="lg"
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" form="schedule-shift" loading={action.pending} disabled={!employee || !site || !ordered}>
            Schedule
          </Button>
        </>
      }
    >
      <form
        id="schedule-shift"
        className="space-y-3"
        onSubmit={(e: FormEvent) => {
          e.preventDefault();
          if (employee && site && ordered) void action.run();
        }}
      >
        <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />
        <EmployeePicker label="Employee" value={employee} onChange={setEmployee} error={fe.employee_id} required />
        <Input
          label="Site"
          value={site}
          onChange={(e) => setSite(e.target.value)}
          list="shift-sites"
          error={fe.site}
          hint="Start typing to pick one of the operating sites."
          required
        />
        <datalist id="shift-sites">
          {siteNames.map((name) => (
            <option key={name} value={name} />
          ))}
        </datalist>
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <Input
            label="Starts"
            type="datetime-local"
            value={startsAt}
            onChange={(e) => setStartsAt(e.target.value)}
            error={fe.starts_at}
            required
          />
          <Input
            label="Ends"
            type="datetime-local"
            value={endsAt}
            onChange={(e) => setEndsAt(e.target.value)}
            error={fe.ends_at ?? (ordered ? undefined : "The end must come after the start")}
            required
          />
        </div>
        <Select
          label="Role on shift"
          options={[
            { value: "", label: "Not specified" },
            { value: "Driver", label: "Driver" },
            { value: "Forklift Operator", label: "Forklift Operator" },
            { value: "Dock Worker", label: "Dock Worker" },
            { value: "Warehouse Handler", label: "Warehouse Handler" },
            { value: "Supervisor", label: "Supervisor" },
          ]}
          value={role}
          onChange={(e) => setRole(e.target.value)}
          error={fe.role_on_shift}
        />
        <p className="text-xs text-slate-500">Times are entered in your local time zone and stored in UTC.</p>
      </form>
    </Modal>
  );
}

function defaultStart(): string {
  const d = new Date();
  d.setHours(d.getHours() + 24, 0, 0, 0);
  return isoToLocalInput(d.toISOString());
}

function defaultEnd(): string {
  const d = new Date();
  d.setHours(d.getHours() + 32, 0, 0, 0);
  return isoToLocalInput(d.toISOString());
}
