"use client";

import { useMemo, useState, type FormEvent } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useMe } from "@/lib/me";
import { useDebounced, useList, useQuery } from "@/lib/hooks";
import { api, asItems } from "@/lib/api";
import { useAction } from "@/lib/forms";
import { fullName, levelName, todayIso } from "@/lib/format";
import { employeeStatusOptions, employmentTypeOptions, levelOptions } from "@/lib/options";
import type { Department, Employee, EmployeeCreateResponse, ListEnvelope, Position } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { DataTable, type Column } from "@/components/ui/DataTable";
import { FilterBar, SearchInput } from "@/components/ui/Filters";
import { Button } from "@/components/ui/Button";
import { Select, Input, FormError } from "@/components/ui/Field";
import { Modal } from "@/components/ui/Modal";
import { StatusBadge } from "@/components/StatusBadge";
import { EmptyState } from "@/components/ui/States";
import { EmployeePicker, type EmployeeOption } from "@/components/pickers/EmployeePicker";
import { OneTimeSecret } from "@/components/OneTimeSecret";

export default function PeoplePage() {
  const { has } = useMe();
  const router = useRouter();
  const [q, setQ] = useState("");
  const [department, setDepartment] = useState("");
  const [status, setStatus] = useState("active");
  const [level, setLevel] = useState("");
  const [creating, setCreating] = useState(false);
  const debouncedQ = useDebounced(q);

  const departments = useQuery<ListEnvelope<Department> | Department[]>("org/departments");
  const departmentOptions = asItems(departments.data).map((d) => ({ value: d.id, label: d.name }));

  const filters = useMemo(
    () => ({ q: debouncedQ, department_id: department, status, level }),
    [debouncedQ, department, status, level],
  );
  const list = useList<Employee>("employees", filters);

  const columns: Column<Employee>[] = [
    { key: "no", header: "No.", render: (e) => <span className="font-mono text-xs">{e.employee_no}</span>, hideOnMobile: true },
    {
      key: "name",
      header: "Name",
      render: (e) => (
        <Link href={`/people/${e.id}`} className="font-medium text-slate-900 hover:text-accent-700">
          {fullName(e)}
        </Link>
      ),
    },
    { key: "title", header: "Title", render: (e) => e.title },
    { key: "dept", header: "Department", render: (e) => e.department_name, hideOnMobile: true },
    { key: "level", header: "Level", render: (e) => levelName(e.level), hideOnMobile: true },
    { key: "status", header: "Status", render: (e) => <StatusBadge status={e.status} /> },
    { key: "site", header: "Site", render: (e) => e.site ?? "", hideOnMobile: true },
  ];

  return (
    <div>
      <PageHeader
        title="People"
        description="Employees within your scope"
        actions={
          has("employees:write:all") ? <Button onClick={() => setCreating(true)}>New employee</Button> : null
        }
      />
      <FilterBar>
        <SearchInput value={q} onChange={setQ} placeholder="Search name, email or number" />
        <Select
          aria-label="Department"
          options={departmentOptions}
          placeholder="All departments"
          value={department}
          onChange={(e) => setDepartment(e.target.value)}
          className="w-full sm:w-52"
        />
        <Select
          aria-label="Status"
          options={employeeStatusOptions}
          placeholder="Any status"
          value={status}
          onChange={(e) => setStatus(e.target.value)}
          className="w-full sm:w-40"
        />
        <Select
          aria-label="Level"
          options={levelOptions}
          placeholder="Any level"
          value={level}
          onChange={(e) => setLevel(e.target.value)}
          className="w-full sm:w-40"
        />
      </FilterBar>
      <DataTable
        columns={columns}
        rows={list.items}
        rowKey={(e) => e.id}
        loading={list.loading}
        error={list.error}
        onRowClick={(e) => router.push(`/people/${e.id}`)}
        pagination={{ page: list.page, perPage: list.perPage, total: list.total, onPage: list.setPage }}
        empty={<EmptyState title="No employees match" description="Try widening the filters. You only see people within your permission scope." />}
      />
      {creating ? (
        <NewEmployeeModal
          onClose={() => setCreating(false)}
          departments={asItems(departments.data)}
          onCreated={() => list.reload()}
        />
      ) : null}
    </div>
  );
}

function NewEmployeeModal({
  onClose,
  departments,
  onCreated,
}: {
  onClose: () => void;
  departments: Department[];
  onCreated: () => void;
}) {
  const positions = useQuery<ListEnvelope<Position> | Position[]>("org/positions");
  const [form, setForm] = useState({
    first_name: "",
    last_name: "",
    email: "",
    phone: "",
    position_id: "",
    department_id: "",
    hire_date: todayIso(),
    employment_type: "full_time",
    site: "",
    base_salary: "",
    currency: "USD",
  });
  const [manager, setManager] = useState<EmployeeOption | null>(null);
  const [created, setCreated] = useState<EmployeeCreateResponse | null>(null);

  const action = useAction(
    () =>
      api.post<EmployeeCreateResponse>("employees", {
        ...form,
        phone: form.phone || null,
        site: form.site || null,
        base_salary: form.base_salary || "0",
        manager_id: manager?.id ?? null,
      }),
    {
      successMessage: "Employee created",
      onSuccess: (res) => {
        setCreated(res);
        onCreated();
      },
    },
  );

  function set<K extends keyof typeof form>(key: K, value: string) {
    setForm((f) => ({ ...f, [key]: value }));
  }

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    void action.run();
  }

  const positionOptions = asItems(positions.data)
    .filter((p) => !form.department_id || !p.department_id || p.department_id === form.department_id)
    .map((p) => ({ value: p.id, label: `${p.title} (L${p.level})` }));
  const fe = action.fieldErrors;

  if (created) {
    return (
      <Modal open onClose={onClose} title="Employee created" size="sm">
        <p className="text-sm text-slate-700">
          {fullName(created)} can sign in with the temporary password below. It is shown once; they must change it on
          first login.
        </p>
        <OneTimeSecret value={created.temporary_password} />
        <div className="mt-4 flex justify-end gap-2">
          <Link href={`/people/${created.id}`} className="text-sm font-medium text-accent-700 hover:underline">
            Open profile
          </Link>
          <Button variant="secondary" onClick={onClose}>
            Done
          </Button>
        </div>
      </Modal>
    );
  }

  return (
    <Modal
      open
      onClose={onClose}
      title="New employee"
      description="Creates the employee record and a user account."
      size="lg"
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" form="new-employee" loading={action.pending}>
            Create
          </Button>
        </>
      }
    >
      <form id="new-employee" onSubmit={onSubmit} className="space-y-3">
        <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <Input label="First name" value={form.first_name} onChange={(e) => set("first_name", e.target.value)} error={fe.first_name} required />
          <Input label="Last name" value={form.last_name} onChange={(e) => set("last_name", e.target.value)} error={fe.last_name} required />
          <Input label="Email" type="email" value={form.email} onChange={(e) => set("email", e.target.value)} error={fe.email} required />
          <Input label="Phone" value={form.phone} onChange={(e) => set("phone", e.target.value)} error={fe.phone} />
          <Select
            label="Department"
            options={departments.map((d) => ({ value: d.id, label: d.name }))}
            placeholder="Choose"
            value={form.department_id}
            onChange={(e) => set("department_id", e.target.value)}
            error={fe.department_id}
            required
          />
          <Select
            label="Position"
            options={positionOptions}
            placeholder="Choose"
            value={form.position_id}
            onChange={(e) => set("position_id", e.target.value)}
            error={fe.position_id}
            required
          />
          <Input label="Hire date" type="date" value={form.hire_date} onChange={(e) => set("hire_date", e.target.value)} error={fe.hire_date} required />
          <Select
            label="Employment type"
            options={employmentTypeOptions}
            value={form.employment_type}
            onChange={(e) => set("employment_type", e.target.value)}
            error={fe.employment_type}
          />
          <Input label="Site" value={form.site} onChange={(e) => set("site", e.target.value)} error={fe.site} />
          <Input
            label="Base salary"
            inputMode="decimal"
            value={form.base_salary}
            onChange={(e) => set("base_salary", e.target.value)}
            error={fe.base_salary}
            hint="Annual, in USD"
          />
        </div>
        <EmployeePicker label="Reports to" value={manager} onChange={setManager} error={fe.manager_id} required />
      </form>
    </Modal>
  );
}
