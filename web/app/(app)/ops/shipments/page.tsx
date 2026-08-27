"use client";

import { Suspense, useMemo, useState, type FormEvent } from "react";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import clsx from "clsx";
import { useMe } from "@/lib/me";
import { useDebounced, useList } from "@/lib/hooks";
import { api } from "@/lib/api";
import { useAction } from "@/lib/forms";
import { formatDate, formatLocation, formatNumber, humanize } from "@/lib/format";
import { incotermOptions, modeOptions, shipmentStatusOptions } from "@/lib/options";
import { SHIPMENT_FLOW } from "@/lib/transitions";
import type { Shipment, ShipmentStatus } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { Card, CardBody } from "@/components/ui/Card";
import { FilterBar, SearchInput } from "@/components/ui/Filters";
import { Button } from "@/components/ui/Button";
import { Checkbox, FormError, Input, Select, Textarea } from "@/components/ui/Field";
import { Modal } from "@/components/ui/Modal";
import { StatusBadge } from "@/components/StatusBadge";
import { EmptyState, ErrorState } from "@/components/ui/States";
import { CardSkeleton, PageSkeleton } from "@/components/ui/Skeleton";
import { Badge } from "@/components/ui/Badge";
import { DelayRisk } from "@/components/DelayRisk";
import { CustomerPicker, type CustomerOption } from "@/components/pickers/CustomerPicker";
import { EmployeePicker, type EmployeeOption } from "@/components/pickers/EmployeePicker";

/** Board columns, in the order a shipment moves through them. */
const BOARD_COLUMNS: ShipmentStatus[] = [...SHIPMENT_FLOW, "exception"];
const BOARD_PAGE_SIZE = 100;

export default function ShipmentsPage() {
  return (
    <Suspense fallback={<PageSkeleton />}>
      <Shipments />
    </Suspense>
  );
}

function Shipments() {
  const params = useSearchParams();
  const router = useRouter();
  const { has } = useMe();
  const [q, setQ] = useState("");
  const [status, setStatus] = useState("");
  const [mode, setMode] = useState("");
  const [customer, setCustomer] = useState<CustomerOption | null>(null);
  const [owner, setOwner] = useState<EmployeeOption | null>(null);
  const [creating, setCreating] = useState(params.get("new") === "1");
  const debouncedQ = useDebounced(q);

  const filters = useMemo(
    () => ({
      q: debouncedQ,
      status,
      mode,
      customer_id: customer?.id ?? "",
      owner_id: owner?.id ?? "",
    }),
    [debouncedQ, status, mode, customer, owner],
  );
  const list = useList<Shipment>("ops/shipments", filters, { perPage: BOARD_PAGE_SIZE });

  const columns = status ? BOARD_COLUMNS.filter((c) => c === status) : BOARD_COLUMNS;
  const grouped = useMemo(() => {
    const map = new Map<ShipmentStatus, Shipment[]>();
    for (const c of BOARD_COLUMNS) map.set(c, []);
    map.set("cancelled", []);
    for (const s of list.items) {
      const bucket = map.get(s.status);
      if (bucket) bucket.push(s);
      else map.set(s.status, [s]);
    }
    return map;
  }, [list.items]);

  const cancelled = grouped.get("cancelled") ?? [];
  const showCancelled = status === "cancelled" || (status === "" && cancelled.length > 0);
  const visibleColumns = status === "cancelled" ? (["cancelled"] as ShipmentStatus[]) : columns;
  const truncated = list.total > list.items.length;

  return (
    <div>
      <PageHeader
        title="Shipments"
        description="Every shipment you can see, grouped by where it is in the state machine."
        actions={has("shipments:write") ? <Button onClick={() => setCreating(true)}>New shipment</Button> : null}
      />

      <FilterBar>
        <SearchInput value={q} onChange={setQ} placeholder="Search reference, cargo or route" />
        <Select
          aria-label="Status"
          options={shipmentStatusOptions}
          placeholder="Any status"
          value={status}
          onChange={(e) => setStatus(e.target.value)}
          className="w-full sm:w-44"
        />
        <Select
          aria-label="Mode"
          options={modeOptions}
          placeholder="Any mode"
          value={mode}
          onChange={(e) => setMode(e.target.value)}
          className="w-full sm:w-36"
        />
        <div className="w-full sm:w-64">
          <CustomerPicker label="" value={customer} onChange={setCustomer} />
        </div>
        <div className="w-full sm:w-64">
          <EmployeePicker label="" value={owner} onChange={setOwner} emptyMessage="No matching coordinator in your scope." />
        </div>
      </FilterBar>

      <div className="mb-3 flex flex-wrap items-center gap-3 text-sm text-slate-600">
        <span>
          {formatNumber(list.total)} {list.total === 1 ? "shipment" : "shipments"} match
        </span>
        {truncated ? (
          <Badge tone="warning">Board shows the first {BOARD_PAGE_SIZE}. Narrow the filters to see the rest.</Badge>
        ) : null}
      </div>

      {list.error ? (
        <ErrorState error={list.error} onRetry={list.reload} />
      ) : list.loading && list.items.length === 0 ? (
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
          <CardSkeleton />
          <CardSkeleton />
          <CardSkeleton />
          <CardSkeleton />
        </div>
      ) : list.items.length === 0 ? (
        <Card>
          <CardBody>
            <EmptyState
              title="No shipments match"
              description="Try clearing a filter. You only see shipments your permissions allow."
            />
          </CardBody>
        </Card>
      ) : (
        <div className="flex snap-x gap-4 overflow-x-auto pb-4">
          {visibleColumns.map((col) => (
            <BoardColumn key={col} status={col} shipments={grouped.get(col) ?? []} />
          ))}
          {showCancelled && status !== "cancelled" ? (
            <BoardColumn status="cancelled" shipments={cancelled} />
          ) : null}
        </div>
      )}

      {creating ? (
        <NewShipmentModal
          onClose={() => setCreating(false)}
          onCreated={(s) => {
            setCreating(false);
            router.push(`/ops/shipments/${s.id}`);
          }}
        />
      ) : null}
    </div>
  );
}

function BoardColumn({ status, shipments }: { status: ShipmentStatus; shipments: Shipment[] }) {
  return (
    <section
      aria-label={humanize(status)}
      className="w-72 shrink-0 snap-start rounded-lg border border-slate-200 bg-white/60 p-3"
    >
      <div className="mb-3 flex items-center justify-between gap-2">
        <StatusBadge status={status} />
        <span className="text-xs font-medium text-slate-500">{shipments.length}</span>
      </div>
      {shipments.length === 0 ? (
        <p className="rounded-md border border-dashed border-slate-200 px-3 py-6 text-center text-xs text-slate-400">
          Nothing here
        </p>
      ) : (
        <ul className="space-y-2">
          {shipments.map((s) => (
            <li key={s.id}>
              <ShipmentCard shipment={s} />
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function ShipmentCard({ shipment }: { shipment: Shipment }) {
  return (
    <Link
      href={`/ops/shipments/${shipment.id}`}
      className={clsx(
        "block rounded-md border border-slate-200 bg-white p-3 shadow-card transition",
        "hover:border-accent-300 hover:shadow-md",
      )}
    >
      <div className="flex items-center justify-between gap-2">
        <span className="font-mono text-xs font-semibold text-slate-900">{shipment.reference}</span>
        <Badge tone="neutral">{shipment.mode}</Badge>
      </div>
      <p className="mt-1 truncate text-sm font-medium text-slate-900">{shipment.customer_name ?? "Customer"}</p>
      <p className="mt-1 text-xs text-slate-600">
        {formatLocation(shipment.origin) || "Origin"} to {formatLocation(shipment.destination) || "destination"}
      </p>
      <div className="mt-2 flex flex-wrap items-center gap-2">
        <span className="text-xs text-slate-500">ETA {shipment.eta ? formatDate(shipment.eta) : "not set"}</span>
        <DelayRisk value={shipment.delay_risk} />
      </div>
      {shipment.owner_name ? <p className="mt-2 truncate text-xs text-slate-500">Owner {shipment.owner_name}</p> : null}
    </Link>
  );
}

// ---------------------------------------------------------------------------
// New shipment
// ---------------------------------------------------------------------------

function NewShipmentModal({ onClose, onCreated }: { onClose: () => void; onCreated: (s: Shipment) => void }) {
  const [customer, setCustomer] = useState<CustomerOption | null>(null);
  const [owner, setOwner] = useState<EmployeeOption | null>(null);
  const [form, setForm] = useState({
    mode: "sea",
    incoterm: "",
    origin_city: "",
    origin_country: "",
    origin_port: "",
    destination_city: "",
    destination_country: "",
    destination_port: "",
    cargo_description: "",
    pieces: "1",
    weight_kg: "",
    volume_cbm: "",
    declared_value: "",
    currency: "USD",
    etd: "",
    eta: "",
  });
  const [hazardous, setHazardous] = useState(false);

  const action = useAction(
    () =>
      api.post<Shipment>("ops/shipments", {
        customer_id: customer?.id,
        mode: form.mode,
        incoterm: form.incoterm || null,
        origin: cleanLocation(form.origin_city, form.origin_country, form.origin_port),
        destination: cleanLocation(form.destination_city, form.destination_country, form.destination_port),
        cargo_description: form.cargo_description,
        pieces: Number(form.pieces) || 0,
        weight_kg: form.weight_kg || "0",
        volume_cbm: form.volume_cbm || null,
        hazardous,
        declared_value: form.declared_value || "0",
        currency: form.currency,
        etd: form.etd || null,
        eta: form.eta || null,
        owner_id: owner?.id ?? null,
      }),
    { successMessage: "Shipment created as a draft", onSuccess: onCreated },
  );

  function set<K extends keyof typeof form>(key: K, value: string) {
    setForm((f) => ({ ...f, [key]: value }));
  }

  const fe = action.fieldErrors;
  const ready = Boolean(customer && form.cargo_description.trim() && form.weight_kg);

  return (
    <Modal
      open
      onClose={onClose}
      title="New shipment"
      description="Creates a draft. The reference is assigned by the API and delay risk is scored once it is booked."
      size="xl"
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" form="new-shipment" loading={action.pending} disabled={!ready}>
            Create draft
          </Button>
        </>
      }
    >
      <form
        id="new-shipment"
        className="space-y-4"
        onSubmit={(e: FormEvent) => {
          e.preventDefault();
          if (ready) void action.run();
        }}
      >
        <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />

        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <CustomerPicker value={customer} onChange={setCustomer} error={fe.customer_id} required />
          <EmployeePicker
            label="Owner"
            value={owner}
            onChange={setOwner}
            error={fe.owner_id}
            hint="The coordinator responsible. Defaults to you."
          />
          <Select
            label="Mode"
            options={modeOptions}
            value={form.mode}
            onChange={(e) => set("mode", e.target.value)}
            error={fe.mode}
            required
          />
          <Select
            label="Incoterm"
            options={incotermOptions}
            placeholder="Not agreed"
            value={form.incoterm}
            onChange={(e) => set("incoterm", e.target.value)}
            error={fe.incoterm}
          />
        </div>

        <fieldset className="rounded-md border border-slate-200 p-3">
          <legend className="px-1 text-xs font-semibold uppercase tracking-wide text-slate-500">Route</legend>
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
            <Input label="Origin city" value={form.origin_city} onChange={(e) => set("origin_city", e.target.value)} error={fe.origin} />
            <Input label="Origin country" value={form.origin_country} onChange={(e) => set("origin_country", e.target.value)} />
            <Input label="Origin port" value={form.origin_port} onChange={(e) => set("origin_port", e.target.value)} />
            <Input label="Destination city" value={form.destination_city} onChange={(e) => set("destination_city", e.target.value)} error={fe.destination} />
            <Input label="Destination country" value={form.destination_country} onChange={(e) => set("destination_country", e.target.value)} />
            <Input label="Destination port" value={form.destination_port} onChange={(e) => set("destination_port", e.target.value)} />
          </div>
        </fieldset>

        <fieldset className="rounded-md border border-slate-200 p-3">
          <legend className="px-1 text-xs font-semibold uppercase tracking-wide text-slate-500">Cargo</legend>
          <Textarea
            label="Description"
            rows={2}
            value={form.cargo_description}
            onChange={(e) => set("cargo_description", e.target.value)}
            error={fe.cargo_description}
            required
          />
          <div className="mt-3 grid grid-cols-1 gap-3 sm:grid-cols-4">
            <Input label="Pieces" inputMode="numeric" value={form.pieces} onChange={(e) => set("pieces", e.target.value)} error={fe.pieces} />
            <Input label="Weight, kg" inputMode="decimal" value={form.weight_kg} onChange={(e) => set("weight_kg", e.target.value)} error={fe.weight_kg} required />
            <Input label="Volume, cbm" inputMode="decimal" value={form.volume_cbm} onChange={(e) => set("volume_cbm", e.target.value)} error={fe.volume_cbm} />
            <Input label="Declared value" inputMode="decimal" value={form.declared_value} onChange={(e) => set("declared_value", e.target.value)} error={fe.declared_value} />
          </div>
          <div className="mt-3">
            <Checkbox label="Hazardous cargo" checked={hazardous} onChange={(e) => setHazardous(e.target.checked)} />
          </div>
        </fieldset>

        <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
          <Input label="ETD" type="date" value={form.etd} onChange={(e) => set("etd", e.target.value)} error={fe.etd} />
          <Input label="ETA" type="date" value={form.eta} onChange={(e) => set("eta", e.target.value)} error={fe.eta} />
          <Input label="Currency" value={form.currency} onChange={(e) => set("currency", e.target.value.toUpperCase())} error={fe.currency} maxLength={3} />
        </div>
      </form>
    </Modal>
  );
}

function cleanLocation(city: string, country: string, port: string): Record<string, string> {
  const out: Record<string, string> = {};
  if (city.trim()) out.city = city.trim();
  if (country.trim()) out.country = country.trim();
  if (port.trim()) out.port = port.trim();
  return out;
}
