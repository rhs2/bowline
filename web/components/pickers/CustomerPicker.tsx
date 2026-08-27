"use client";

import { api } from "@/lib/api";
import type { Customer, ListEnvelope } from "@/lib/types";
import { SearchPicker } from "./SearchPicker";

export interface CustomerOption {
  id: string;
  code: string;
  name: string;
}

export function toCustomerOption(c: Pick<Customer, "id" | "code" | "name">): CustomerOption {
  return { id: c.id, code: c.code, name: c.name };
}

export async function searchCustomers(q: string): Promise<CustomerOption[]> {
  const data = await api.get<ListEnvelope<Customer> | Customer[]>("ops/customers", {
    query: { q, per_page: 20 },
  });
  const items = Array.isArray(data) ? data : data.items;
  return items.map(toCustomerOption);
}

export function CustomerPicker({
  value,
  onChange,
  label = "Customer",
  error,
  hint,
  required,
  disabled,
}: {
  value: CustomerOption | null;
  onChange: (value: CustomerOption | null) => void;
  label?: string;
  error?: string;
  hint?: string;
  required?: boolean;
  disabled?: boolean;
}) {
  return (
    <SearchPicker<CustomerOption>
      label={label}
      placeholder="Search by customer name or code"
      search={searchCustomers}
      getKey={(c) => c.id}
      getLabel={(c) => c.name}
      renderOption={(c) => (
        <span className="flex flex-col">
          <span className="font-medium">{c.name}</span>
          <span className="font-mono text-xs text-slate-500">{c.code}</span>
        </span>
      )}
      value={value ? [value] : []}
      onChange={(items) => onChange(items[0] ?? null)}
      emptyMessage="No matching customer. Customers are created under Operations, Customers."
      error={error}
      hint={hint}
      required={required}
      disabled={disabled}
    />
  );
}
