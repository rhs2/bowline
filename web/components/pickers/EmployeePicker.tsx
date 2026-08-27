"use client";

import { api, type Query } from "@/lib/api";
import { fullName } from "@/lib/format";
import type { Employee, ListEnvelope } from "@/lib/types";
import { SearchPicker } from "./SearchPicker";

export interface EmployeeOption {
  id: string;
  name: string;
  title: string;
  department: string;
}

export function toOption(e: Employee): EmployeeOption {
  return { id: e.id, name: fullName(e), title: e.title, department: e.department_name };
}

/** Scoped employee search (GET /employees); the API only returns people in the caller's scope. */
export function searchEmployees(filters: Query = {}) {
  return async (q: string): Promise<EmployeeOption[]> => {
    const data = await api.get<ListEnvelope<Employee>>("employees", {
      query: { q, status: "active", per_page: 20, ...filters },
    });
    return data.items.map(toOption);
  };
}

export function EmployeePicker({
  value,
  onChange,
  label = "Employee",
  error,
  hint,
  filters,
  required,
  disabled,
  emptyMessage,
}: {
  value: EmployeeOption | null;
  onChange: (value: EmployeeOption | null) => void;
  label?: string;
  error?: string;
  hint?: string;
  filters?: Query;
  required?: boolean;
  disabled?: boolean;
  emptyMessage?: string;
}) {
  return (
    <SearchPicker<EmployeeOption>
      label={label}
      placeholder="Search by name or employee number"
      search={searchEmployees(filters)}
      getKey={(e) => e.id}
      getLabel={(e) => e.name}
      renderOption={(e) => (
        <span className="flex flex-col">
          <span className="font-medium">{e.name}</span>
          <span className="text-xs text-slate-500">
            {e.title}
            {e.department ? `, ${e.department}` : ""}
          </span>
        </span>
      )}
      value={value ? [value] : []}
      onChange={(items) => onChange(items[0] ?? null)}
      emptyMessage={
        emptyMessage ??
        "No matching employee within your scope. You only see people who report up to you, or everyone if you hold a company-wide permission."
      }
      error={error}
      hint={hint}
      required={required}
      disabled={disabled}
    />
  );
}
