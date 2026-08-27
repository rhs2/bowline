"use client";

import { useState, type FormEvent } from "react";
import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { useMe } from "@/lib/me";
import { useQuery } from "@/lib/hooks";
import { api, asItems } from "@/lib/api";
import { useAction } from "@/lib/forms";
import { formatDate, formatMoney, fullName, levelName, todayIso } from "@/lib/format";
import { employeeStatusOptions, employmentTypeOptions } from "@/lib/options";
import type { ChainLink, Employee, EmployeeDetail, EmployeePatch, ListEnvelope, Position } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { Card, CardBody, CardHeader, DescriptionList } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { Input, Select, FormError } from "@/components/ui/Field";
import { Modal } from "@/components/ui/Modal";
import { StatusBadge } from "@/components/StatusBadge";
import { PageSkeleton } from "@/components/ui/Skeleton";
import { ErrorState, EmptyState } from "@/components/ui/States";
import { Avatar } from "@/components/ui/Avatar";
import { EmployeePicker, toOption, type EmployeeOption } from "@/components/pickers/EmployeePicker";

export default function EmployeePage() {
  const params = useParams<{ id: string }>();
  const id = params.id;
  const router = useRouter();
  const { employee: me, can, has } = useMe();
  const detail = useQuery<EmployeeDetail>(`employees/${id}`);
  const reports = useQuery<ListEnvelope<Employee> | Employee[]>(`employees/${id}/reports`);
  const chain = useQuery<ListEnvelope<ChainLink> | ChainLink[]>(`employees/${id}/chain`);
  const [editing, setEditing] = useState(false);
  const [reparenting, setReparenting] = useState(false);
  const [terminating, setTerminating] = useState(false);

  const e = detail.data;
  const canWrite = can("employees:write:subtree");
  const canTerminate = has("employees:write:all");
  const isSelf = me?.id === id;

  if (detail.loading && !e) return <PageSkeleton />;
  if (detail.error) {
    return (
      <div>
        <PageHeader title="Employee" backHref="/people" backLabel="People" />
        <ErrorState error={detail.error} onRetry={detail.reload} />
      </div>
    );
  }
  if (!e) return null;

  const reportRows = asItems(reports.data);
  const chainRows = [...asItems(chain.data)].sort((a, b) => a.level - b.level);

  return (
    <div>
      <PageHeader
        title={
          <span className="flex items-center gap-3">
            <Avatar name={fullName(e)} size="lg" />
            {fullName(e)}
          </span>
        }
        description={`${e.title}, ${e.department_name}`}
        backHref="/people"
        backLabel="People"
        meta={<StatusBadge status={e.status} />}
        actions={
          <>
            {canWrite && e.status !== "terminated" ? (
              <>
                <Button variant="secondary" onClick={() => setReparenting(true)}>
                  Change manager
                </Button>
                <Button variant="secondary" onClick={() => setEditing(true)}>
                  Edit
                </Button>
              </>
            ) : null}
            {canTerminate && e.status !== "terminated" && !isSelf ? (
              <Button variant="danger" onClick={() => setTerminating(true)}>
                Terminate
              </Button>
            ) : null}
          </>
        }
      />

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
        <div className="space-y-4 lg:col-span-2">
          <Card>
            <CardHeader title="Profile" />
            <CardBody>
              <DescriptionList
                columns={2}
                items={[
                  { label: "Employee number", value: <span className="font-mono">{e.employee_no}</span> },
                  { label: "Email", value: <a href={`mailto:${e.email}`} className="hover:text-accent-700">{e.email}</a> },
                  { label: "Phone", value: e.phone },
                  { label: "Level", value: `${e.level}, ${levelName(e.level)}` },
                  { label: "Employment type", value: e.employment_type.replace("_", " ") },
                  { label: "Hire date", value: formatDate(e.hire_date) },
                  { label: "Site", value: e.site },
                  { label: "Pay grade", value: e.pay_grade },
                  ...(e.base_salary !== undefined
                    ? [{ label: "Base salary", value: formatMoney(e.base_salary, e.currency ?? "USD") }]
                    : []),
                  ...(e.termination_date ? [{ label: "Termination date", value: formatDate(e.termination_date) }] : []),
                ]}
              />
            </CardBody>
          </Card>

          <Card>
            <CardHeader
              title="Direct reports"
              description={`${e.direct_reports_count} ${e.direct_reports_count === 1 ? "person reports" : "people report"} to ${e.first_name}`}
            />
            <CardBody>
              {reports.error ? (
                <ErrorState error={reports.error} onRetry={reports.reload} />
              ) : reportRows.length === 0 ? (
                <EmptyState title="No direct reports" />
              ) : (
                <ul className="divide-y divide-slate-100">
                  {reportRows.map((r) => (
                    <li key={r.id} className="flex items-center justify-between gap-3 py-2">
                      <Link href={`/people/${r.id}`} className="min-w-0 hover:text-accent-700">
                        <span className="block truncate text-sm font-medium text-slate-900">{fullName(r)}</span>
                        <span className="block truncate text-xs text-slate-500">{r.title}</span>
                      </Link>
                      <StatusBadge status={r.status} />
                    </li>
                  ))}
                </ul>
              )}
            </CardBody>
          </Card>
        </div>

        <div className="space-y-4">
          <Card>
            <CardHeader title="Reports to" />
            <CardBody>
              {e.manager ? (
                <Link href={`/people/${e.manager.id}`} className="flex items-center gap-3 hover:text-accent-700">
                  <Avatar name={e.manager.name} />
                  <span className="min-w-0">
                    <span className="block truncate text-sm font-medium text-slate-900">{e.manager.name}</span>
                    <span className="block truncate text-xs text-slate-500">{e.manager.title}</span>
                  </span>
                </Link>
              ) : (
                <p className="text-sm text-slate-500">No manager: this is the top of the tree.</p>
              )}
            </CardBody>
          </Card>
          <Card>
            <CardHeader title="Chain of command" />
            <CardBody>
              {chainRows.length === 0 ? (
                <p className="text-sm text-slate-500">Not available.</p>
              ) : (
                <ol className="space-y-2">
                  {chainRows.map((link, i) => (
                    <li key={link.id} className="text-sm" style={{ paddingLeft: `${i * 0.75}rem` }}>
                      <Link href={`/people/${link.id}`} className="hover:text-accent-700">
                        <span className="font-medium text-slate-900">{link.name}</span>
                        <span className="block text-xs text-slate-500">{link.title}</span>
                      </Link>
                    </li>
                  ))}
                </ol>
              )}
            </CardBody>
          </Card>
        </div>
      </div>

      {editing ? <EditModal employee={e} onClose={() => setEditing(false)} onSaved={detail.reload} /> : null}
      {reparenting ? (
        <ReparentModal employee={e} onClose={() => setReparenting(false)} onSaved={() => { detail.reload(); chain.reload(); }} />
      ) : null}
      {terminating ? (
        <TerminateModal
          employee={e}
          onClose={() => setTerminating(false)}
          onDone={() => {
            detail.reload();
            reports.reload();
            router.refresh();
          }}
        />
      ) : null}
    </div>
  );
}

function EditModal({ employee, onClose, onSaved }: { employee: EmployeeDetail; onClose: () => void; onSaved: () => void }) {
  const positions = useQuery<ListEnvelope<Position> | Position[]>("org/positions");
  const [form, setForm] = useState({
    first_name: employee.first_name,
    last_name: employee.last_name,
    phone: employee.phone ?? "",
    position_id: employee.position_id,
    status: employee.status,
    employment_type: employee.employment_type,
    site: employee.site ?? "",
    pay_grade: employee.pay_grade ?? "",
    base_salary: employee.base_salary ?? "",
  });
  const action = useAction(
    () => {
      const patch: EmployeePatch = {
        first_name: form.first_name,
        last_name: form.last_name,
        phone: form.phone || null,
        position_id: form.position_id,
        status: form.status,
        employment_type: form.employment_type,
        site: form.site || null,
        pay_grade: form.pay_grade || null,
      };
      if (employee.base_salary !== undefined && form.base_salary !== "") patch.base_salary = form.base_salary;
      return api.patch<EmployeeDetail>(`employees/${employee.id}`, patch);
    },
    { successMessage: "Employee updated", onSuccess: () => { onSaved(); onClose(); } },
  );
  function set<K extends keyof typeof form>(key: K, value: (typeof form)[K]) {
    setForm((f) => ({ ...f, [key]: value }));
  }
  const fe = action.fieldErrors;
  const positionOptions = asItems(positions.data).map((p) => ({ value: p.id, label: `${p.title} (L${p.level})` }));
  if (!positionOptions.some((p) => p.value === form.position_id)) {
    positionOptions.unshift({ value: form.position_id, label: employee.title });
  }
  return (
    <Modal
      open
      onClose={onClose}
      title={`Edit ${fullName(employee)}`}
      size="lg"
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>Cancel</Button>
          <Button type="submit" form="edit-employee" loading={action.pending}>Save</Button>
        </>
      }
    >
      <form
        id="edit-employee"
        className="space-y-3"
        onSubmit={(e: FormEvent) => {
          e.preventDefault();
          void action.run();
        }}
      >
        <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <Input label="First name" value={form.first_name} onChange={(e) => set("first_name", e.target.value)} error={fe.first_name} required />
          <Input label="Last name" value={form.last_name} onChange={(e) => set("last_name", e.target.value)} error={fe.last_name} required />
          <Input label="Phone" value={form.phone} onChange={(e) => set("phone", e.target.value)} error={fe.phone} />
          <Select label="Position" options={positionOptions} value={form.position_id} onChange={(e) => set("position_id", e.target.value)} error={fe.position_id} />
          <Select
            label="Status"
            options={employeeStatusOptions.filter((o) => o.value !== "terminated")}
            value={form.status}
            onChange={(e) => set("status", e.target.value as EmployeeDetail["status"])}
            error={fe.status}
            hint="Use Terminate for leavers so tokens are revoked and reports re-parented."
          />
          <Select
            label="Employment type"
            options={employmentTypeOptions}
            value={form.employment_type}
            onChange={(e) => set("employment_type", e.target.value as EmployeeDetail["employment_type"])}
            error={fe.employment_type}
          />
          <Input label="Site" value={form.site} onChange={(e) => set("site", e.target.value)} error={fe.site} />
          <Input label="Pay grade" value={form.pay_grade} onChange={(e) => set("pay_grade", e.target.value)} error={fe.pay_grade} />
          {employee.base_salary !== undefined ? (
            <Input label="Base salary" inputMode="decimal" value={form.base_salary} onChange={(e) => set("base_salary", e.target.value)} error={fe.base_salary} />
          ) : null}
        </div>
      </form>
    </Modal>
  );
}

function ReparentModal({ employee, onClose, onSaved }: { employee: EmployeeDetail; onClose: () => void; onSaved: () => void }) {
  const [manager, setManager] = useState<EmployeeOption | null>(
    employee.manager ? { id: employee.manager.id, name: employee.manager.name, title: employee.manager.title ?? "", department: "" } : null,
  );
  const action = useAction(
    () => api.patch<EmployeeDetail>(`employees/${employee.id}`, { manager_id: manager?.id ?? null }),
    { successMessage: "Manager updated", onSuccess: () => { onSaved(); onClose(); } },
  );
  return (
    <Modal
      open
      onClose={onClose}
      title="Change manager"
      description={`Moves ${employee.first_name} and everyone below them under the new manager. Cycles are rejected.`}
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>Cancel</Button>
          <Button onClick={() => void action.run()} loading={action.pending} disabled={!manager || manager.id === employee.id}>Move</Button>
        </>
      }
    >
      <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />
      <EmployeePicker label="New manager" value={manager} onChange={setManager} error={action.fieldErrors.manager_id} required />
    </Modal>
  );
}

function TerminateModal({ employee, onClose, onDone }: { employee: EmployeeDetail; onClose: () => void; onDone: () => void }) {
  const [date, setDate] = useState(todayIso());
  const [reassign, setReassign] = useState<EmployeeOption | null>(null);
  const action = useAction(
    () =>
      api.post<EmployeeDetail>(`employees/${employee.id}/terminate`, {
        termination_date: date,
        reassign_reports_to: reassign?.id ?? undefined,
      }),
    { successMessage: "Employee terminated", onSuccess: () => { onDone(); onClose(); } },
  );
  return (
    <Modal
      open
      onClose={onClose}
      title={`Terminate ${fullName(employee)}`}
      description="Disables sign-in, revokes sessions and re-parents their reports. This cannot be undone from the UI."
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>Cancel</Button>
          <Button variant="danger" onClick={() => void action.run()} loading={action.pending}>Terminate</Button>
        </>
      }
    >
      <div className="space-y-3">
        <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />
        <Input label="Termination date" type="date" value={date} onChange={(e) => setDate(e.target.value)} error={action.fieldErrors.termination_date} required />
        <EmployeePicker
          label="Reassign direct reports to"
          value={reassign}
          onChange={setReassign}
          hint={employee.manager ? `Defaults to their manager, ${employee.manager.name}.` : "Defaults to their manager."}
          error={action.fieldErrors.reassign_reports_to}
        />
      </div>
    </Modal>
  );
}

export { toOption };
