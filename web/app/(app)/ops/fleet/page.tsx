"use client";

import { useMemo, useState, type FormEvent } from "react";
import { useMe } from "@/lib/me";
import { useDebounced, useList, useQuery } from "@/lib/hooks";
import { api, asItems } from "@/lib/api";
import { useAction } from "@/lib/forms";
import { formatNumber, formatPercent, humanize } from "@/lib/format";
import { modeOptions, siteKindOptions, vehicleKindOptions, vehicleStatusOptions } from "@/lib/options";
import type { Carrier, ListEnvelope, Site, Vehicle } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { DataTable, type Column } from "@/components/ui/DataTable";
import { FilterBar, SearchInput } from "@/components/ui/Filters";
import { Tabs } from "@/components/ui/Tabs";
import { Button } from "@/components/ui/Button";
import { Checkbox, FormError, Input, Select } from "@/components/ui/Field";
import { Modal } from "@/components/ui/Modal";
import { StatusBadge } from "@/components/StatusBadge";
import { Badge } from "@/components/ui/Badge";
import { EmptyState } from "@/components/ui/States";
import { EmployeePicker, type EmployeeOption } from "@/components/pickers/EmployeePicker";

type Tab = "carriers" | "sites" | "vehicles";

export default function FleetPage() {
  const { has } = useMe();
  const canManage = has("fleet:manage");
  const [tab, setTab] = useState<Tab>("carriers");

  return (
    <div>
      <PageHeader
        title="Fleet"
        description="Carriers we book, the sites we operate from, and the vehicles on the yard."
      />
      <div className="mb-4">
        <Tabs<Tab>
          tabs={[
            { key: "carriers", label: "Carriers" },
            { key: "sites", label: "Sites" },
            { key: "vehicles", label: "Vehicles" },
          ]}
          value={tab}
          onChange={setTab}
        />
      </div>
      {tab === "carriers" ? <CarriersTab canManage={canManage} /> : null}
      {tab === "sites" ? <SitesTab canManage={canManage} /> : null}
      {tab === "vehicles" ? <VehiclesTab canManage={canManage} /> : null}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Carriers
// ---------------------------------------------------------------------------

function CarriersTab({ canManage }: { canManage: boolean }) {
  const [q, setQ] = useState("");
  const [mode, setMode] = useState("");
  const [editing, setEditing] = useState<Carrier | null>(null);
  const [creating, setCreating] = useState(false);
  const debouncedQ = useDebounced(q);

  const filters = useMemo(() => ({ q: debouncedQ, mode }), [debouncedQ, mode]);
  const list = useList<Carrier>("ops/carriers", filters);

  const columns: Column<Carrier>[] = [
    { key: "code", header: "Code", render: (c) => <span className="font-mono text-xs">{c.code}</span> },
    { key: "name", header: "Name", render: (c) => <span className="font-medium text-slate-900">{c.name}</span> },
    { key: "mode", header: "Mode", render: (c) => <Badge tone="neutral">{c.mode}</Badge> },
    { key: "scac", header: "SCAC", render: (c) => c.scac ?? "", hideOnMobile: true },
    {
      key: "ontime",
      header: "On time",
      align: "right",
      render: (c) => (c.on_time_rate === null || c.on_time_rate === undefined ? "" : formatPercent(c.on_time_rate)),
    },
    { key: "active", header: "Active", render: (c) => <StatusBadge status={c.active ? "active" : "retired"} label={c.active ? "Active" : "Inactive"} /> },
    ...(canManage
      ? [
          {
            key: "actions",
            header: "",
            align: "right" as const,
            render: (c: Carrier) => (
              <Button variant="secondary" size="sm" onClick={() => setEditing(c)}>
                Edit
              </Button>
            ),
          },
        ]
      : []),
  ];

  return (
    <>
      <FilterBar>
        <SearchInput value={q} onChange={setQ} placeholder="Search carrier name or code" />
        <Select
          aria-label="Mode"
          options={modeOptions}
          placeholder="Any mode"
          value={mode}
          onChange={(e) => setMode(e.target.value)}
          className="w-full sm:w-40"
        />
        {canManage ? (
          <Button className="sm:ml-auto" onClick={() => setCreating(true)}>
            New carrier
          </Button>
        ) : null}
      </FilterBar>
      <DataTable
        columns={columns}
        rows={list.items}
        rowKey={(c) => c.id}
        loading={list.loading}
        error={list.error}
        pagination={{ page: list.page, perPage: list.perPage, total: list.total, onPage: list.setPage }}
        empty={<EmptyState title="No carriers" description="Carriers are the lines and hauliers we book capacity with." />}
      />
      {creating || editing ? (
        <CarrierModal
          carrier={editing}
          onClose={() => {
            setCreating(false);
            setEditing(null);
          }}
          onSaved={() => {
            setCreating(false);
            setEditing(null);
            list.reload();
          }}
        />
      ) : null}
    </>
  );
}

function CarrierModal({
  carrier,
  onClose,
  onSaved,
}: {
  carrier: Carrier | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [form, setForm] = useState({
    code: carrier?.code ?? "",
    name: carrier?.name ?? "",
    mode: String(carrier?.mode ?? "sea"),
    scac: carrier?.scac ?? "",
    email: carrier?.contact?.email ?? "",
    phone: carrier?.contact?.phone ?? "",
  });
  const [active, setActive] = useState(carrier?.active ?? true);

  const action = useAction(
    () => {
      const body = {
        code: form.code,
        name: form.name,
        mode: form.mode,
        scac: form.scac || null,
        contact: { email: form.email, phone: form.phone },
        active,
      };
      return carrier ? api.patch<Carrier>(`ops/carriers/${carrier.id}`, body) : api.post<Carrier>("ops/carriers", body);
    },
    { successMessage: carrier ? "Carrier updated" : "Carrier added", onSuccess: onSaved },
  );

  function set<K extends keyof typeof form>(key: K, value: string) {
    setForm((f) => ({ ...f, [key]: value }));
  }

  const fe = action.fieldErrors;
  const ready = Boolean(form.code.trim() && form.name.trim());

  return (
    <Modal
      open
      onClose={onClose}
      title={carrier ? `Edit ${carrier.name}` : "New carrier"}
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" form="carrier-form" loading={action.pending} disabled={!ready}>
            {carrier ? "Save changes" : "Add carrier"}
          </Button>
        </>
      }
    >
      <form
        id="carrier-form"
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
          <Select label="Mode" options={modeOptions} value={form.mode} onChange={(e) => set("mode", e.target.value)} error={fe.mode} required />
          <Input label="SCAC" value={form.scac} onChange={(e) => set("scac", e.target.value.toUpperCase())} error={fe.scac} maxLength={4} hint="Standard carrier alpha code, if they have one." />
          <Input label="Contact email" type="email" value={form.email} onChange={(e) => set("email", e.target.value)} />
          <Input label="Contact phone" value={form.phone} onChange={(e) => set("phone", e.target.value)} />
        </div>
        <Checkbox label="Available for booking" checked={active} onChange={(e) => setActive(e.target.checked)} />
      </form>
    </Modal>
  );
}

// ---------------------------------------------------------------------------
// Sites
// ---------------------------------------------------------------------------

function SitesTab({ canManage }: { canManage: boolean }) {
  const [q, setQ] = useState("");
  const [editing, setEditing] = useState<Site | null>(null);
  const [creating, setCreating] = useState(false);
  const debouncedQ = useDebounced(q);

  const filters = useMemo(() => ({ q: debouncedQ }), [debouncedQ]);
  const list = useList<Site>("ops/sites", filters);

  const columns: Column<Site>[] = [
    { key: "code", header: "Code", render: (s) => <span className="font-mono text-xs">{s.code}</span> },
    { key: "name", header: "Name", render: (s) => <span className="font-medium text-slate-900">{s.name}</span> },
    { key: "kind", header: "Kind", render: (s) => humanize(s.kind) },
    {
      key: "address",
      header: "Address",
      hideOnMobile: true,
      render: (s) => [s.address?.city, s.address?.country].filter(Boolean).join(", "),
    },
    ...(canManage
      ? [
          {
            key: "actions",
            header: "",
            align: "right" as const,
            render: (s: Site) => (
              <Button variant="secondary" size="sm" onClick={() => setEditing(s)}>
                Edit
              </Button>
            ),
          },
        ]
      : []),
  ];

  return (
    <>
      <FilterBar>
        <SearchInput value={q} onChange={setQ} placeholder="Search site name or code" />
        {canManage ? (
          <Button className="sm:ml-auto" onClick={() => setCreating(true)}>
            New site
          </Button>
        ) : null}
      </FilterBar>
      <DataTable
        columns={columns}
        rows={list.items}
        rowKey={(s) => s.id}
        loading={list.loading}
        error={list.error}
        pagination={{ page: list.page, perPage: list.perPage, total: list.total, onPage: list.setPage }}
        empty={<EmptyState title="No sites" description="Sites are the depots, ports and offices we work from." />}
      />
      {creating || editing ? (
        <SiteModal
          site={editing}
          onClose={() => {
            setCreating(false);
            setEditing(null);
          }}
          onSaved={() => {
            setCreating(false);
            setEditing(null);
            list.reload();
          }}
        />
      ) : null}
    </>
  );
}

function SiteModal({ site, onClose, onSaved }: { site: Site | null; onClose: () => void; onSaved: () => void }) {
  const [form, setForm] = useState({
    code: site?.code ?? "",
    name: site?.name ?? "",
    kind: String(site?.kind ?? "warehouse"),
    line1: site?.address?.line1 ?? "",
    city: site?.address?.city ?? "",
    country: site?.address?.country ?? "",
  });
  const [manager, setManager] = useState<EmployeeOption | null>(null);

  const action = useAction(
    () => {
      const body = {
        code: form.code,
        name: form.name,
        kind: form.kind,
        address: { line1: form.line1 || undefined, city: form.city || undefined, country: form.country || undefined },
        manager_id: manager?.id ?? site?.manager_id ?? null,
      };
      return site ? api.patch<Site>(`ops/sites/${site.id}`, body) : api.post<Site>("ops/sites", body);
    },
    { successMessage: site ? "Site updated" : "Site added", onSuccess: onSaved },
  );

  function set<K extends keyof typeof form>(key: K, value: string) {
    setForm((f) => ({ ...f, [key]: value }));
  }

  const fe = action.fieldErrors;
  const ready = Boolean(form.code.trim() && form.name.trim());

  return (
    <Modal
      open
      onClose={onClose}
      title={site ? `Edit ${site.name}` : "New site"}
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" form="site-form" loading={action.pending} disabled={!ready}>
            {site ? "Save changes" : "Add site"}
          </Button>
        </>
      }
    >
      <form
        id="site-form"
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
          <Select label="Kind" options={siteKindOptions} value={form.kind} onChange={(e) => set("kind", e.target.value)} error={fe.kind} required />
          <Input label="Address" value={form.line1} onChange={(e) => set("line1", e.target.value)} />
          <Input label="City" value={form.city} onChange={(e) => set("city", e.target.value)} />
          <Input label="Country" value={form.country} onChange={(e) => set("country", e.target.value)} />
        </div>
        <EmployeePicker
          label="Site manager"
          value={manager}
          onChange={setManager}
          error={fe.manager_id}
          hint={site?.manager_id && !manager ? "Leave blank to keep the current manager." : undefined}
        />
      </form>
    </Modal>
  );
}

// ---------------------------------------------------------------------------
// Vehicles
// ---------------------------------------------------------------------------

function VehiclesTab({ canManage }: { canManage: boolean }) {
  const [q, setQ] = useState("");
  const [status, setStatus] = useState("");
  const [editing, setEditing] = useState<Vehicle | null>(null);
  const [creating, setCreating] = useState(false);
  const debouncedQ = useDebounced(q);

  const sites = useQuery<ListEnvelope<Site> | Site[]>("ops/sites");
  const siteRows = asItems(sites.data);
  const siteName = (id: string | null) => siteRows.find((s) => s.id === id)?.name ?? "";

  const filters = useMemo(() => ({ q: debouncedQ, status }), [debouncedQ, status]);
  const list = useList<Vehicle>("ops/vehicles", filters);

  const columns: Column<Vehicle>[] = [
    { key: "plate", header: "Plate", render: (v) => <span className="font-mono text-xs font-semibold">{v.plate}</span> },
    { key: "kind", header: "Kind", render: (v) => humanize(v.kind) },
    {
      key: "capacity",
      header: "Capacity",
      align: "right",
      render: (v) => (v.capacity_kg === null || v.capacity_kg === undefined ? "" : `${formatNumber(v.capacity_kg)} kg`),
    },
    { key: "home", header: "Home site", render: (v) => siteName(v.home_site_id), hideOnMobile: true },
    { key: "status", header: "Status", render: (v) => <StatusBadge status={v.status} /> },
    ...(canManage
      ? [
          {
            key: "actions",
            header: "",
            align: "right" as const,
            render: (v: Vehicle) => (
              <Button variant="secondary" size="sm" onClick={() => setEditing(v)}>
                Edit
              </Button>
            ),
          },
        ]
      : []),
  ];

  return (
    <>
      <FilterBar>
        <SearchInput value={q} onChange={setQ} placeholder="Search plate" />
        <Select
          aria-label="Status"
          options={vehicleStatusOptions}
          placeholder="Any status"
          value={status}
          onChange={(e) => setStatus(e.target.value)}
          className="w-full sm:w-44"
        />
        {canManage ? (
          <Button className="sm:ml-auto" onClick={() => setCreating(true)}>
            New vehicle
          </Button>
        ) : null}
      </FilterBar>
      <DataTable
        columns={columns}
        rows={list.items}
        rowKey={(v) => v.id}
        loading={list.loading}
        error={list.error}
        pagination={{ page: list.page, perPage: list.perPage, total: list.total, onPage: list.setPage }}
        empty={<EmptyState title="No vehicles" description="Trucks, vans, trailers and forklifts on the yard." />}
      />
      {creating || editing ? (
        <VehicleModal
          vehicle={editing}
          sites={siteRows}
          onClose={() => {
            setCreating(false);
            setEditing(null);
          }}
          onSaved={() => {
            setCreating(false);
            setEditing(null);
            list.reload();
          }}
        />
      ) : null}
    </>
  );
}

function VehicleModal({
  vehicle,
  sites,
  onClose,
  onSaved,
}: {
  vehicle: Vehicle | null;
  sites: Site[];
  onClose: () => void;
  onSaved: () => void;
}) {
  const [form, setForm] = useState({
    plate: vehicle?.plate ?? "",
    kind: String(vehicle?.kind ?? "truck"),
    capacity_kg: vehicle?.capacity_kg === null || vehicle?.capacity_kg === undefined ? "" : String(vehicle.capacity_kg),
    status: String(vehicle?.status ?? "available"),
    home_site_id: vehicle?.home_site_id ?? "",
  });

  const action = useAction(
    () => {
      const body = {
        plate: form.plate,
        kind: form.kind,
        capacity_kg: form.capacity_kg || null,
        status: form.status,
        home_site_id: form.home_site_id || null,
      };
      return vehicle ? api.patch<Vehicle>(`ops/vehicles/${vehicle.id}`, body) : api.post<Vehicle>("ops/vehicles", body);
    },
    { successMessage: vehicle ? "Vehicle updated" : "Vehicle added", onSuccess: onSaved },
  );

  function set<K extends keyof typeof form>(key: K, value: string) {
    setForm((f) => ({ ...f, [key]: value }));
  }

  const fe = action.fieldErrors;
  const ready = Boolean(form.plate.trim());

  return (
    <Modal
      open
      onClose={onClose}
      title={vehicle ? `Edit ${vehicle.plate}` : "New vehicle"}
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" form="vehicle-form" loading={action.pending} disabled={!ready}>
            {vehicle ? "Save changes" : "Add vehicle"}
          </Button>
        </>
      }
    >
      <form
        id="vehicle-form"
        className="space-y-3"
        onSubmit={(e: FormEvent) => {
          e.preventDefault();
          if (ready) void action.run();
        }}
      >
        <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <Input label="Plate" value={form.plate} onChange={(e) => set("plate", e.target.value.toUpperCase())} error={fe.plate} required />
          <Select label="Kind" options={vehicleKindOptions} value={form.kind} onChange={(e) => set("kind", e.target.value)} error={fe.kind} required />
          <Input label="Capacity, kg" inputMode="decimal" value={form.capacity_kg} onChange={(e) => set("capacity_kg", e.target.value)} error={fe.capacity_kg} />
          <Select label="Status" options={vehicleStatusOptions} value={form.status} onChange={(e) => set("status", e.target.value)} error={fe.status} />
          <Select
            label="Home site"
            options={sites.map((s) => ({ value: s.id, label: s.name }))}
            placeholder="Not assigned"
            value={form.home_site_id}
            onChange={(e) => set("home_site_id", e.target.value)}
            error={fe.home_site_id}
            className="sm:col-span-2"
          />
        </div>
      </form>
    </Modal>
  );
}
