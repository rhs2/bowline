"use client";

import { useMemo, useState, type FormEvent } from "react";
import Link from "next/link";
import { useMe } from "@/lib/me";
import { useDebounced, useList, useQuery } from "@/lib/hooks";
import { api } from "@/lib/api";
import { useAction } from "@/lib/forms";
import { formatDate, formatMoney } from "@/lib/format";
import { customerStatusOptions } from "@/lib/options";
import type { Address, Customer, CustomerStatus } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { DataTable, type Column } from "@/components/ui/DataTable";
import { FilterBar, SearchInput } from "@/components/ui/Filters";
import { Button } from "@/components/ui/Button";
import { FormError, Input, Select } from "@/components/ui/Field";
import { Modal } from "@/components/ui/Modal";
import { StatusBadge } from "@/components/StatusBadge";
import { DescriptionList } from "@/components/ui/Card";
import { EmptyState, ErrorState } from "@/components/ui/States";
import { Skeleton } from "@/components/ui/Skeleton";
import { EmployeePicker, type EmployeeOption } from "@/components/pickers/EmployeePicker";

function formatAddress(a: Address | null | undefined): string {
  if (!a) return "";
  return [a.line1, a.line2, a.city, a.region, a.postal_code, a.country].filter(Boolean).join(", ");
}

export default function CustomersPage() {
  const { has } = useMe();
  const canManage = has("customers:manage");
  const [q, setQ] = useState("");
  const [status, setStatus] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const debouncedQ = useDebounced(q);

  const filters = useMemo(() => ({ q: debouncedQ, status }), [debouncedQ, status]);
  const list = useList<Customer>("ops/customers", filters);

  const columns: Column<Customer>[] = [
    { key: "code", header: "Code", render: (c) => <span className="font-mono text-xs">{c.code}</span> },
    {
      key: "name",
      header: "Name",
      render: (c) => (
        <button type="button" className="text-left font-medium text-slate-900 hover:text-accent-700" onClick={() => setSelectedId(c.id)}>
          {c.name}
        </button>
      ),
    },
    { key: "contact", header: "Contact", render: (c) => c.contact_name ?? "", hideOnMobile: true },
    { key: "email", header: "Email", render: (c) => c.contact_email ?? "", hideOnMobile: true },
    { key: "manager", header: "Account manager", render: (c) => c.account_manager?.name ?? "", hideOnMobile: true },
    { key: "credit", header: "Credit limit", align: "right", render: (c) => formatMoney(c.credit_limit, c.currency) },
    { key: "status", header: "Status", render: (c) => <StatusBadge status={c.status} /> },
  ];

  return (
    <div>
      <PageHeader
        title="Customers"
        description="Accounts you ship for, with their billing details and credit limits."
        actions={canManage ? <Button onClick={() => setCreating(true)}>New customer</Button> : null}
      />

      <FilterBar>
        <SearchInput value={q} onChange={setQ} placeholder="Search name or code" />
        <Select
          aria-label="Status"
          options={customerStatusOptions}
          placeholder="Any status"
          value={status}
          onChange={(e) => setStatus(e.target.value)}
          className="w-full sm:w-40"
        />
      </FilterBar>

      <DataTable
        columns={columns}
        rows={list.items}
        rowKey={(c) => c.id}
        loading={list.loading}
        error={list.error}
        onRowClick={(c) => setSelectedId(c.id)}
        pagination={{ page: list.page, perPage: list.perPage, total: list.total, onPage: list.setPage }}
        empty={<EmptyState title="No customers match" description="Try a different search term." />}
      />

      {selectedId ? (
        <CustomerDrawer
          id={selectedId}
          canManage={canManage}
          onClose={() => setSelectedId(null)}
          onSaved={() => list.reload()}
        />
      ) : null}
      {creating ? (
        <CustomerFormModal
          customer={null}
          onClose={() => setCreating(false)}
          onSaved={(c) => {
            setCreating(false);
            list.reload();
            setSelectedId(c.id);
          }}
        />
      ) : null}
    </div>
  );
}

function CustomerDrawer({
  id,
  canManage,
  onClose,
  onSaved,
}: {
  id: string;
  canManage: boolean;
  onClose: () => void;
  onSaved: () => void;
}) {
  const detail = useQuery<Customer>(`ops/customers/${id}`);
  const [editing, setEditing] = useState(false);
  const c = detail.data;

  if (editing && c) {
    return (
      <CustomerFormModal
        customer={c}
        onClose={() => setEditing(false)}
        onSaved={() => {
          setEditing(false);
          detail.reload();
          onSaved();
        }}
      />
    );
  }

  return (
    <Modal
      open
      onClose={onClose}
      title={c?.name ?? "Customer"}
      description={c ? `${c.code}, on the books since ${formatDate(c.created_at)}` : undefined}
      size="lg"
      footer={
        <>
          <Link href="/ops/shipments" className="mr-auto text-sm font-medium text-accent-700 hover:underline">
            Go to shipments
          </Link>
          <Button variant="secondary" onClick={onClose}>
            Close
          </Button>
          {canManage && c ? <Button onClick={() => setEditing(true)}>Edit</Button> : null}
        </>
      }
    >
      {detail.loading && !c ? (
        <div className="space-y-2">
          <Skeleton className="h-4 w-2/3" />
          <Skeleton className="h-4 w-1/2" />
          <Skeleton className="h-4 w-3/4" />
        </div>
      ) : detail.error ? (
        <ErrorState error={detail.error} onRetry={detail.reload} />
      ) : c ? (
        <DescriptionList
          columns={2}
          items={[
            { label: "Status", value: <StatusBadge status={c.status} /> },
            { label: "Code", value: <span className="font-mono">{c.code}</span> },
            { label: "Contact", value: c.contact_name },
            {
              label: "Email",
              value: c.contact_email ? (
                <a href={`mailto:${c.contact_email}`} className="hover:text-accent-700">
                  {c.contact_email}
                </a>
              ) : null,
            },
            { label: "Phone", value: c.phone },
            { label: "Account manager", value: c.account_manager?.name ?? null },
            { label: "Credit limit", value: formatMoney(c.credit_limit, c.currency) },
            { label: "Currency", value: c.currency },
            { label: "Billing address", value: formatAddress(c.billing_address) },
          ]}
        />
      ) : null}
    </Modal>
  );
}

interface CustomerForm {
  code: string;
  name: string;
  contact_name: string;
  contact_email: string;
  phone: string;
  line1: string;
  line2: string;
  city: string;
  region: string;
  postal_code: string;
  country: string;
  credit_limit: string;
  currency: string;
  status: CustomerStatus;
}

function toForm(c: Customer | null): CustomerForm {
  const a = c?.billing_address ?? {};
  return {
    code: c?.code ?? "",
    name: c?.name ?? "",
    contact_name: c?.contact_name ?? "",
    contact_email: c?.contact_email ?? "",
    phone: c?.phone ?? "",
    line1: a.line1 ?? "",
    line2: a.line2 ?? "",
    city: a.city ?? "",
    region: a.region ?? "",
    postal_code: a.postal_code ?? "",
    country: a.country ?? "",
    credit_limit: c?.credit_limit ?? "0.00",
    currency: c?.currency ?? "USD",
    status: c?.status ?? "active",
  };
}

function CustomerFormModal({
  customer,
  onClose,
  onSaved,
}: {
  customer: Customer | null;
  onClose: () => void;
  onSaved: (c: Customer) => void;
}) {
  const [form, setForm] = useState<CustomerForm>(() => toForm(customer));
  const [manager, setManager] = useState<EmployeeOption | null>(
    customer?.account_manager
      ? { id: customer.account_manager.id, name: customer.account_manager.name, title: customer.account_manager.title ?? "", department: "" }
      : null,
  );

  const action = useAction(
    () => {
      const body = {
        code: form.code,
        name: form.name,
        contact_name: form.contact_name || null,
        contact_email: form.contact_email || null,
        phone: form.phone || null,
        billing_address: {
          line1: form.line1 || undefined,
          line2: form.line2 || undefined,
          city: form.city || undefined,
          region: form.region || undefined,
          postal_code: form.postal_code || undefined,
          country: form.country || undefined,
        },
        credit_limit: form.credit_limit || "0",
        currency: form.currency,
        status: form.status,
        account_manager_id: manager?.id ?? null,
      };
      return customer
        ? api.patch<Customer>(`ops/customers/${customer.id}`, body)
        : api.post<Customer>("ops/customers", body);
    },
    { successMessage: customer ? "Customer updated" : "Customer created", onSuccess: onSaved },
  );

  function set<K extends keyof CustomerForm>(key: K, value: CustomerForm[K]) {
    setForm((f) => ({ ...f, [key]: value }));
  }

  const fe = action.fieldErrors;
  const ready = Boolean(form.code.trim() && form.name.trim());

  return (
    <Modal
      open
      onClose={onClose}
      title={customer ? `Edit ${customer.name}` : "New customer"}
      size="lg"
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" form="customer-form" loading={action.pending} disabled={!ready}>
            {customer ? "Save changes" : "Create customer"}
          </Button>
        </>
      }
    >
      <form
        id="customer-form"
        className="space-y-3"
        onSubmit={(e: FormEvent) => {
          e.preventDefault();
          if (ready) void action.run();
        }}
      >
        <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <Input label="Code" value={form.code} onChange={(e) => set("code", e.target.value.toUpperCase())} error={fe.code} required />
          <Input label="Name" value={form.name} onChange={(e) => set("name", e.target.value)} error={fe.name} required />
          <Input label="Contact name" value={form.contact_name} onChange={(e) => set("contact_name", e.target.value)} error={fe.contact_name} />
          <Input label="Contact email" type="email" value={form.contact_email} onChange={(e) => set("contact_email", e.target.value)} error={fe.contact_email} />
          <Input label="Phone" value={form.phone} onChange={(e) => set("phone", e.target.value)} error={fe.phone} />
          <Select
            label="Status"
            options={customerStatusOptions}
            value={form.status}
            onChange={(e) => set("status", e.target.value as CustomerStatus)}
            error={fe.status}
          />
        </div>

        <fieldset className="rounded-md border border-slate-200 p-3">
          <legend className="px-1 text-xs font-semibold uppercase tracking-wide text-slate-500">Billing address</legend>
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <Input label="Line 1" value={form.line1} onChange={(e) => set("line1", e.target.value)} />
            <Input label="Line 2" value={form.line2} onChange={(e) => set("line2", e.target.value)} />
            <Input label="City" value={form.city} onChange={(e) => set("city", e.target.value)} />
            <Input label="Region" value={form.region} onChange={(e) => set("region", e.target.value)} />
            <Input label="Postal code" value={form.postal_code} onChange={(e) => set("postal_code", e.target.value)} />
            <Input label="Country" value={form.country} onChange={(e) => set("country", e.target.value)} />
          </div>
        </fieldset>

        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <Input
            label="Credit limit"
            inputMode="decimal"
            value={form.credit_limit}
            onChange={(e) => set("credit_limit", e.target.value)}
            error={fe.credit_limit}
          />
          <Input label="Currency" maxLength={3} value={form.currency} onChange={(e) => set("currency", e.target.value.toUpperCase())} error={fe.currency} />
        </div>
        <EmployeePicker label="Account manager" value={manager} onChange={setManager} error={fe.account_manager_id} />
      </form>
    </Modal>
  );
}
