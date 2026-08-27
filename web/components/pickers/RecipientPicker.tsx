"use client";

import { api } from "@/lib/api";
import type { ListEnvelope, Recipient } from "@/lib/types";
import { SearchPicker } from "./SearchPicker";

/** GET /comms/recipients returns only the people the caller may write to. */
export async function fetchRecipients(q: string): Promise<Recipient[]> {
  const data = await api.get<ListEnvelope<Recipient> | Recipient[]>("comms/recipients", {
    query: { q, per_page: 20 },
  });
  return Array.isArray(data) ? data : data.items;
}

/**
 * Plain-language version of the messaging rules from docs/DOMAIN.md, shown whenever
 * a search comes back empty so nobody wonders why a colleague is missing.
 */
export function MessagingRules({ query }: { query?: string }) {
  return (
    <div className="space-y-1">
      <p className="font-medium text-slate-700">
        {query ? `Nobody matching "${query}" is in your reach.` : "No recipients available."}
      </p>
      <p>
        You can message your manager, your direct reports, anyone in your department, and the
        Service Desk. Supervisors and managers can also reach everyone who reports up to them.
        Anyone else is out of scope for direct messages; open a support ticket if you need to
        reach another team.
      </p>
    </div>
  );
}

export function RecipientPicker({
  value,
  onChange,
  error,
  search = fetchRecipients,
  label = "To",
  multiple = true,
}: {
  value: Recipient[];
  onChange: (value: Recipient[]) => void;
  error?: string;
  search?: (q: string) => Promise<Recipient[]>;
  label?: string;
  multiple?: boolean;
}) {
  return (
    <SearchPicker<Recipient>
      label={label}
      placeholder="Search people you can message"
      search={search}
      getKey={(r) => r.id}
      getLabel={(r) => r.name}
      renderOption={(r) => (
        <span className="flex flex-col">
          <span className="font-medium">{r.name}</span>
          <span className="text-xs text-slate-500">
            {r.title}
            {r.department ? `, ${r.department}` : ""}
          </span>
        </span>
      )}
      value={value}
      onChange={onChange}
      multiple={multiple}
      emptyMessage={<MessagingRules />}
      error={error}
      required
    />
  );
}
