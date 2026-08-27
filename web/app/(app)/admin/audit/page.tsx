"use client";

import { useMemo, useState } from "react";
import { useList } from "@/lib/hooks";
import { addDays, formatDateTime, humanize, todayIso } from "@/lib/format";
import type { AuditEntry } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { Card, CardBody } from "@/components/ui/Card";
import { DataTable, type Column } from "@/components/ui/DataTable";
import { FilterBar } from "@/components/ui/Filters";
import { Button } from "@/components/ui/Button";
import { Input, Select } from "@/components/ui/Field";
import { Modal } from "@/components/ui/Modal";
import { Badge } from "@/components/ui/Badge";
import { EmptyState } from "@/components/ui/States";
import { JsonView } from "@/components/JsonView";
import { EmployeePicker, type EmployeeOption } from "@/components/pickers/EmployeePicker";

/** Entity types the API writes audit rows for, from docs/DOMAIN.md. */
const ENTITY_TYPES = [
  "employee",
  "user",
  "leave_request",
  "shift",
  "attendance",
  "employee_document",
  "shipment",
  "shipment_leg",
  "work_order",
  "customer",
  "carrier",
  "site",
  "vehicle",
  "invoice",
  "payment",
  "expense",
  "journal_entry",
  "payroll_run",
  "fiscal_period",
  "ticket",
].map((v) => ({ value: v, label: humanize(v) }));

const DEFAULT_WINDOW_DAYS = 30;

export default function AuditPage() {
  const today = todayIso();
  const [entityType, setEntityType] = useState("");
  const [entityId, setEntityId] = useState("");
  const [actor, setActor] = useState<EmployeeOption | null>(null);
  const [from, setFrom] = useState(addDays(today, -DEFAULT_WINDOW_DAYS));
  const [to, setTo] = useState(today);
  const [inspecting, setInspecting] = useState<AuditEntry | null>(null);

  const filters = useMemo(
    () => ({ entity_type: entityType, entity_id: entityId.trim(), actor: actor?.id ?? "", from, to }),
    [entityType, entityId, actor, from, to],
  );
  const list = useList<AuditEntry>("admin/audit", filters, { perPage: 50 });

  const columns: Column<AuditEntry>[] = [
    { key: "at", header: "When", render: (e) => <span className="whitespace-nowrap">{formatDateTime(e.at)}</span> },
    { key: "actor", header: "Actor", render: (e) => e.actor_name ?? <span className="text-slate-400">System</span> },
    { key: "action", header: "Action", render: (e) => <Badge tone="accent">{humanize(e.action)}</Badge> },
    { key: "entity", header: "Entity", render: (e) => humanize(e.entity_type) },
    {
      key: "entity_id",
      header: "Entity id",
      hideOnMobile: true,
      render: (e) => <span className="font-mono text-xs text-slate-600">{e.entity_id ?? ""}</span>,
    },
    { key: "ip", header: "IP", render: (e) => e.ip ?? "", hideOnMobile: true },
    {
      key: "request",
      header: "Request",
      hideOnMobile: true,
      render: (e) => <span className="font-mono text-xs text-slate-500">{e.request_id ?? ""}</span>,
    },
    {
      key: "actions",
      header: "",
      align: "right",
      render: (e) => (
        <Button variant="ghost" size="sm" onClick={() => setInspecting(e)}>
          Before and after
        </Button>
      ),
    },
  ];

  function clearFilters() {
    setEntityType("");
    setEntityId("");
    setActor(null);
    setFrom(addDays(today, -DEFAULT_WINDOW_DAYS));
    setTo(today);
  }

  return (
    <div>
      <PageHeader
        title="Audit log"
        description="Every change the API made, written in the same transaction as the change itself."
      />

      <FilterBar>
        <Select
          aria-label="Entity type"
          options={ENTITY_TYPES}
          placeholder="Any entity"
          value={entityType}
          onChange={(e) => setEntityType(e.target.value)}
          className="w-full sm:w-48"
        />
        <Input
          aria-label="Entity id"
          placeholder="Entity id"
          value={entityId}
          onChange={(e) => setEntityId(e.target.value)}
          className="w-full sm:w-72"
        />
        <div className="w-full sm:w-64">
          <EmployeePicker label="" value={actor} onChange={setActor} emptyMessage="No matching person in your scope." />
        </div>
        <Input aria-label="From" type="date" value={from} onChange={(e) => setFrom(e.target.value)} className="w-full sm:w-44" />
        <Input aria-label="To" type="date" value={to} onChange={(e) => setTo(e.target.value)} className="w-full sm:w-44" />
        <Button variant="secondary" onClick={clearFilters}>
          Reset
        </Button>
      </FilterBar>

      <DataTable
        columns={columns}
        rows={list.items}
        rowKey={(e) => String(e.id)}
        loading={list.loading}
        error={list.error}
        dense
        pagination={{ page: list.page, perPage: list.perPage, total: list.total, onPage: list.setPage }}
        empty={
          <EmptyState
            title="No audit rows"
            description="Nothing was recorded for this filter. Widen the date range or clear the entity filter."
          />
        }
      />

      <Card className="mt-4">
        <CardBody className="py-3">
          <p className="text-xs text-slate-500">
            Rows are immutable. Filtering by entity id is the quickest way to reconstruct the history of a single record.
          </p>
        </CardBody>
      </Card>

      {inspecting ? (
        <Modal
          open
          onClose={() => setInspecting(null)}
          title={`${humanize(inspecting.action)} on ${humanize(inspecting.entity_type)}`}
          description={`${formatDateTime(inspecting.at)}${inspecting.actor_name ? `, by ${inspecting.actor_name}` : ""}`}
          size="xl"
          footer={
            <Button variant="secondary" onClick={() => setInspecting(null)}>
              Close
            </Button>
          }
        >
          <div className="space-y-4">
            <div>
              <p className="mb-1 text-xs font-semibold uppercase tracking-wide text-slate-500">Before</p>
              <JsonView value={inspecting.before} />
            </div>
            <div>
              <p className="mb-1 text-xs font-semibold uppercase tracking-wide text-slate-500">After</p>
              <JsonView value={inspecting.after} />
            </div>
            <dl className="grid grid-cols-2 gap-3 text-xs text-slate-600">
              <div>
                <dt className="font-semibold uppercase tracking-wide text-slate-500">Entity id</dt>
                <dd className="mt-0.5 break-all font-mono">{inspecting.entity_id ?? "none"}</dd>
              </div>
              <div>
                <dt className="font-semibold uppercase tracking-wide text-slate-500">Request id</dt>
                <dd className="mt-0.5 break-all font-mono">{inspecting.request_id ?? "none"}</dd>
              </div>
            </dl>
          </div>
        </Modal>
      ) : null}
    </div>
  );
}
