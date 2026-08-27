"use client";

import { useState, type FormEvent } from "react";
import Link from "next/link";
import { useParams } from "next/navigation";
import { useMe } from "@/lib/me";
import { useQuery } from "@/lib/hooks";
import { api, asItems } from "@/lib/api";
import { useAction } from "@/lib/forms";
import {
  formatDate,
  formatDateTime,
  formatLocation,
  formatMoney,
  formatNumber,
  humanize,
  localInputToIso,
} from "@/lib/format";
import { modeOptions, shipmentDocumentKindOptions, shipmentEventOptions } from "@/lib/options";
import { shipmentTransitions } from "@/lib/transitions";
import { statusTone } from "@/components/StatusBadge";
import type {
  Carrier,
  ListEnvelope,
  ShipmentDetail,
  ShipmentEventType,
  ShipmentLeg,
  ShipmentStatus,
  TransportMode,
  Vehicle,
  WorkOrder,
} from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { Card, CardBody, CardHeader, DescriptionList } from "@/components/ui/Card";
import { DataTable, type Column } from "@/components/ui/DataTable";
import { Button } from "@/components/ui/Button";
import { FormError, Input, Select, Textarea } from "@/components/ui/Field";
import { Modal } from "@/components/ui/Modal";
import { StatusBadge } from "@/components/StatusBadge";
import { Badge } from "@/components/ui/Badge";
import { PageSkeleton } from "@/components/ui/Skeleton";
import { EmptyState, ErrorState } from "@/components/ui/States";
import { Timeline, type TimelineItem } from "@/components/Timeline";
import { DelayRisk } from "@/components/DelayRisk";
import { DocumentList } from "@/components/DocumentList";
import { UploadDocument } from "@/components/UploadDocument";
import { EmployeePicker, type EmployeeOption } from "@/components/pickers/EmployeePicker";

export default function ShipmentPage() {
  const { id } = useParams<{ id: string }>();
  const { has } = useMe();
  const detail = useQuery<ShipmentDetail>(`ops/shipments/${id}`);
  const [transition, setTransition] = useState<ShipmentStatus | null>(null);
  const [addingLeg, setAddingLeg] = useState(false);
  const [addingEvent, setAddingEvent] = useState(false);

  const s = detail.data;
  const canWrite = has("shipments:write");
  const canAssign = has("shipments:assign");

  if (detail.loading && !s) return <PageSkeleton />;
  if (detail.error) {
    return (
      <div>
        <PageHeader title="Shipment" backHref="/ops/shipments" backLabel="Shipments" />
        <ErrorState error={detail.error} onRetry={detail.reload} />
      </div>
    );
  }
  if (!s) return null;

  const nextStates = canWrite ? shipmentTransitions(s.status, s.previous_status) : [];
  const events = [...s.events].sort((a, b) => b.occurred_at.localeCompare(a.occurred_at));
  const legs = [...s.legs].sort((a, b) => a.seq - b.seq);

  const timeline: TimelineItem[] = events.map((e) => ({
    key: e.id,
    title: humanize(e.event_type),
    time: formatDateTime(e.occurred_at),
    tone: statusTone(e.event_type),
    body: (
      <>
        {e.location ? <p className="text-slate-700">{e.location}</p> : null}
        {e.note ? <p className="whitespace-pre-wrap">{e.note}</p> : null}
        {e.recorded_by_name ? <p className="mt-0.5 text-xs text-slate-500">Recorded by {e.recorded_by_name}</p> : null}
      </>
    ),
  }));

  const legColumns: Column<ShipmentLeg>[] = [
    { key: "seq", header: "Leg", render: (l) => l.seq },
    { key: "mode", header: "Mode", render: (l) => <Badge tone="neutral">{l.mode}</Badge> },
    {
      key: "route",
      header: "From and to",
      render: (l) => (
        <span>
          {formatLocation(l.from_location) || "Origin"} to {formatLocation(l.to_location) || "destination"}
        </span>
      ),
    },
    { key: "carrier", header: "Carrier", render: (l) => l.carrier_name ?? "", hideOnMobile: true },
    {
      key: "vehicle",
      header: "Vehicle and driver",
      hideOnMobile: true,
      render: (l) => (
        <span className="text-xs text-slate-600">
          {[l.vehicle_plate, l.driver_name].filter(Boolean).join(", ") || "Not assigned"}
        </span>
      ),
    },
    {
      key: "planned",
      header: "Planned",
      hideOnMobile: true,
      render: (l) => (
        <span className="text-xs text-slate-600">
          {l.planned_departure ? formatDateTime(l.planned_departure) : "?"} to{" "}
          {l.planned_arrival ? formatDateTime(l.planned_arrival) : "?"}
        </span>
      ),
    },
    {
      key: "actual",
      header: "Actual",
      hideOnMobile: true,
      render: (l) => (
        <span className="text-xs text-slate-600">
          {l.actual_departure ? formatDateTime(l.actual_departure) : "not departed"}
          {l.actual_arrival ? ` to ${formatDateTime(l.actual_arrival)}` : ""}
        </span>
      ),
    },
    { key: "status", header: "Status", render: (l) => <StatusBadge status={l.status} /> },
  ];

  const workOrderColumns: Column<WorkOrder>[] = [
    { key: "title", header: "Task", render: (w) => <span className="font-medium text-slate-900">{w.title}</span> },
    { key: "kind", header: "Kind", render: (w) => humanize(w.kind) },
    { key: "assignee", header: "Assigned to", render: (w) => w.assigned_to_name ?? "Unassigned", hideOnMobile: true },
    { key: "due", header: "Due", render: (w) => (w.due_at ? formatDateTime(w.due_at) : ""), hideOnMobile: true },
    { key: "status", header: "Status", render: (w) => <StatusBadge status={w.status} /> },
  ];

  return (
    <div>
      <PageHeader
        title={s.reference}
        description={`${s.customer_name ?? "Customer"}, ${formatLocation(s.origin) || "origin"} to ${formatLocation(s.destination) || "destination"}`}
        backHref="/ops/shipments"
        backLabel="Shipments"
        meta={
          <>
            <StatusBadge status={s.status} />
            <Badge tone="neutral">{s.mode}</Badge>
            <DelayRisk value={s.delay_risk} />
          </>
        }
        actions={
          nextStates.length > 0 ? (
            nextStates.map((to) => (
              <Button
                key={to}
                variant={to === "delivered" ? "success" : to === "cancelled" || to === "exception" ? "secondary" : "primary"}
                onClick={() => setTransition(to)}
              >
                {transitionLabel(s.status, to)}
              </Button>
            ))
          ) : (
            <span className="text-sm text-slate-500">
              {s.status === "delivered" || s.status === "cancelled"
                ? "This shipment is closed."
                : "You cannot move this shipment."}
            </span>
          )
        }
      />

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
        <div className="space-y-4 lg:col-span-2">
          <Card>
            <CardHeader title="Cargo and schedule" />
            <CardBody>
              <DescriptionList
                columns={3}
                items={[
                  { label: "Description", value: s.cargo_description },
                  { label: "Pieces", value: formatNumber(s.pieces) },
                  { label: "Weight", value: `${formatNumber(s.weight_kg, 2)} kg` },
                  { label: "Volume", value: s.volume_cbm ? `${formatNumber(s.volume_cbm, 3)} cbm` : null },
                  { label: "Hazardous", value: s.hazardous ? <Badge tone="warning">Hazardous</Badge> : "No" },
                  { label: "Declared value", value: formatMoney(s.declared_value, s.currency) },
                  { label: "Incoterm", value: s.incoterm },
                  { label: "ETD", value: s.etd ? formatDate(s.etd) : null },
                  { label: "ETA", value: s.eta ? formatDate(s.eta) : null },
                  { label: "Delivered", value: s.delivered_at ? formatDateTime(s.delivered_at) : null },
                  { label: "Owner", value: s.owner_name },
                  {
                    label: "Invoice",
                    value: s.invoice ? (
                      <Link href={`/finance/invoices/${s.invoice.id}`} className="font-medium text-accent-700 hover:underline">
                        {s.invoice.invoice_no}, {formatMoney(s.invoice.total, s.invoice.currency)}
                      </Link>
                    ) : null,
                  },
                ]}
              />
            </CardBody>
          </Card>

          <Card>
            <CardHeader
              title="Legs"
              description="Ordered segments from origin to destination."
              actions={canAssign ? <Button size="sm" onClick={() => setAddingLeg(true)}>Add leg</Button> : null}
            />
            <CardBody className="px-0 py-0">
              <DataTable
                columns={legColumns}
                rows={legs}
                rowKey={(l) => l.id}
                dense
                empty={
                  <EmptyState
                    title="No legs planned"
                    description={canAssign ? "Add the first segment to start routing this shipment." : "A dispatcher has not routed this shipment yet."}
                  />
                }
              />
            </CardBody>
          </Card>

          <Card>
            <CardHeader
              title="Work orders"
              description="Ground tasks raised against this shipment."
              actions={
                <Link href="/ops/work-orders" className="text-sm font-medium text-accent-700 hover:underline">
                  All work orders
                </Link>
              }
            />
            <CardBody className="px-0 py-0">
              <DataTable
                columns={workOrderColumns}
                rows={s.work_orders}
                rowKey={(w) => w.id}
                dense
                empty={<EmptyState title="No work orders" description="Nothing has been raised for the ground crew." />}
              />
            </CardBody>
          </Card>

          <Card>
            <CardHeader title="Documents" description="Bills of lading, customs paperwork and proof of delivery." />
            <CardBody className="space-y-4">
              <DocumentList
                documents={s.documents}
                downloadPath={(d) => `ops/shipments/${s.id}/documents/${d.id}/download`}
                empty={<EmptyState title="No documents" description="Nothing has been filed against this shipment." />}
              />
              {canWrite ? (
                <div className="border-t border-slate-200 pt-4">
                  <UploadDocument
                    spec={{
                      presignPath: `ops/shipments/${s.id}/documents/presign`,
                      confirmPath: `ops/shipments/${s.id}/documents`,
                      extra: { shipment_id: s.id },
                      kinds: shipmentDocumentKindOptions,
                    }}
                    onDone={() => detail.reload()}
                  />
                </div>
              ) : null}
            </CardBody>
          </Card>
        </div>

        <div className="space-y-4">
          <Card>
            <CardHeader
              title="Tracking"
              description={`${events.length} ${events.length === 1 ? "event" : "events"}, newest first`}
              actions={canWrite ? <Button size="sm" variant="secondary" onClick={() => setAddingEvent(true)}>Add event</Button> : null}
            />
            <CardBody>
              {timeline.length === 0 ? (
                <EmptyState title="No events yet" description="Tracking events appear here as the shipment moves." />
              ) : (
                <Timeline items={timeline} />
              )}
            </CardBody>
          </Card>
        </div>
      </div>

      {transition ? (
        <TransitionModal
          shipmentId={s.id}
          from={s.status}
          to={transition}
          onClose={() => setTransition(null)}
          onDone={() => {
            setTransition(null);
            detail.reload();
          }}
        />
      ) : null}
      {addingLeg ? (
        <AddLegModal
          shipmentId={s.id}
          nextSeq={(legs[legs.length - 1]?.seq ?? 0) + 1}
          defaultMode={s.mode}
          onClose={() => setAddingLeg(false)}
          onDone={() => {
            setAddingLeg(false);
            detail.reload();
          }}
        />
      ) : null}
      {addingEvent ? (
        <AddEventModal
          shipmentId={s.id}
          onClose={() => setAddingEvent(false)}
          onDone={() => {
            setAddingEvent(false);
            detail.reload();
          }}
        />
      ) : null}
    </div>
  );
}

function transitionLabel(from: ShipmentStatus, to: ShipmentStatus): string {
  if (to === "cancelled") return "Cancel";
  if (to === "exception") return "Flag exception";
  if (from === "exception") return `Resume as ${humanize(to).toLowerCase()}`;
  return `Move to ${humanize(to).toLowerCase()}`;
}

// ---------------------------------------------------------------------------
// Transition
// ---------------------------------------------------------------------------

function TransitionModal({
  shipmentId,
  from,
  to,
  onClose,
  onDone,
}: {
  shipmentId: string;
  from: ShipmentStatus;
  to: ShipmentStatus;
  onClose: () => void;
  onDone: () => void;
}) {
  const [note, setNote] = useState("");
  const [location, setLocation] = useState("");
  const action = useAction(
    () => api.post(`ops/shipments/${shipmentId}/transition`, { to, note: note || undefined, location: location || undefined }),
    { successMessage: `Shipment moved to ${humanize(to).toLowerCase()}`, onSuccess: onDone },
  );
  const noteRequired = to === "exception" || to === "cancelled";

  return (
    <Modal
      open
      onClose={onClose}
      title={transitionLabel(from, to)}
      description={`${humanize(from)} to ${humanize(to).toLowerCase()}. The API records a tracking event for this change.`}
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button
            variant={to === "cancelled" ? "danger" : "primary"}
            loading={action.pending}
            disabled={noteRequired && !note.trim()}
            onClick={() => void action.run()}
          >
            Confirm
          </Button>
        </>
      }
    >
      <div className="space-y-3">
        <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />
        <Input
          label="Location"
          value={location}
          onChange={(e) => setLocation(e.target.value)}
          error={action.fieldErrors.location}
          hint="Where the shipment is right now. Optional."
        />
        <Textarea
          label="Note"
          rows={3}
          value={note}
          onChange={(e) => setNote(e.target.value)}
          error={action.fieldErrors.note}
          required={noteRequired}
          hint={noteRequired ? "Explain why. This is kept on the tracking timeline." : "Optional."}
        />
      </div>
    </Modal>
  );
}

// ---------------------------------------------------------------------------
// Add a leg
// ---------------------------------------------------------------------------

function AddLegModal({
  shipmentId,
  nextSeq,
  defaultMode,
  onClose,
  onDone,
}: {
  shipmentId: string;
  nextSeq: number;
  defaultMode: TransportMode;
  onClose: () => void;
  onDone: () => void;
}) {
  const carriers = useQuery<ListEnvelope<Carrier> | Carrier[]>("ops/carriers");
  const vehicles = useQuery<ListEnvelope<Vehicle> | Vehicle[]>("ops/vehicles");
  const [driver, setDriver] = useState<EmployeeOption | null>(null);
  const [form, setForm] = useState({
    seq: String(nextSeq),
    mode: String(defaultMode),
    carrier_id: "",
    vehicle_id: "",
    from_city: "",
    from_country: "",
    to_city: "",
    to_country: "",
    planned_departure: "",
    planned_arrival: "",
  });

  const action = useAction(
    () =>
      api.post(`ops/shipments/${shipmentId}/legs`, {
        seq: Number(form.seq) || nextSeq,
        mode: form.mode,
        carrier_id: form.carrier_id || null,
        vehicle_id: form.vehicle_id || null,
        driver_id: driver?.id ?? null,
        from: locationOf(form.from_city, form.from_country),
        to: locationOf(form.to_city, form.to_country),
        planned_departure: localInputToIso(form.planned_departure) || null,
        planned_arrival: localInputToIso(form.planned_arrival) || null,
      }),
    { successMessage: "Leg added", onSuccess: onDone },
  );

  function set<K extends keyof typeof form>(key: K, value: string) {
    setForm((f) => ({ ...f, [key]: value }));
  }

  const carrierOptions = asItems(carriers.data)
    .filter((c) => c.active && (!form.mode || c.mode === form.mode))
    .map((c) => ({ value: c.id, label: `${c.name} (${c.code})` }));
  const vehicleOptions = asItems(vehicles.data)
    .filter((v) => v.status !== "retired")
    .map((v) => ({ value: v.id, label: `${v.plate}, ${humanize(v.kind)}` }));
  const fe = action.fieldErrors;

  return (
    <Modal
      open
      onClose={onClose}
      title={`Add leg ${form.seq}`}
      description="Legs are ordered segments. Carriers are filtered to the leg's mode."
      size="lg"
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" form="add-leg" loading={action.pending}>
            Add leg
          </Button>
        </>
      }
    >
      <form
        id="add-leg"
        className="space-y-3"
        onSubmit={(e: FormEvent) => {
          e.preventDefault();
          void action.run();
        }}
      >
        <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <Input label="Sequence" inputMode="numeric" value={form.seq} onChange={(e) => set("seq", e.target.value)} error={fe.seq} required />
          <Select label="Mode" options={modeOptions} value={form.mode} onChange={(e) => set("mode", e.target.value)} error={fe.mode} required />
          <Select
            label="Carrier"
            options={carrierOptions}
            placeholder={carrierOptions.length === 0 ? "No carriers for this mode" : "Choose a carrier"}
            value={form.carrier_id}
            onChange={(e) => set("carrier_id", e.target.value)}
            error={fe.carrier_id}
          />
          <Select
            label="Vehicle"
            options={vehicleOptions}
            placeholder="Not assigned"
            value={form.vehicle_id}
            onChange={(e) => set("vehicle_id", e.target.value)}
            error={fe.vehicle_id}
          />
          <Input label="From city" value={form.from_city} onChange={(e) => set("from_city", e.target.value)} error={fe.from} />
          <Input label="From country" value={form.from_country} onChange={(e) => set("from_country", e.target.value)} />
          <Input label="To city" value={form.to_city} onChange={(e) => set("to_city", e.target.value)} error={fe.to} />
          <Input label="To country" value={form.to_country} onChange={(e) => set("to_country", e.target.value)} />
          <Input
            label="Planned departure"
            type="datetime-local"
            value={form.planned_departure}
            onChange={(e) => set("planned_departure", e.target.value)}
            error={fe.planned_departure}
          />
          <Input
            label="Planned arrival"
            type="datetime-local"
            value={form.planned_arrival}
            onChange={(e) => set("planned_arrival", e.target.value)}
            error={fe.planned_arrival}
          />
        </div>
        <EmployeePicker
          label="Driver"
          value={driver}
          onChange={setDriver}
          error={fe.driver_id}
          emptyMessage="No matching driver in your scope. Drivers are ground-level employees who report up to you."
        />
      </form>
    </Modal>
  );
}

function locationOf(city: string, country: string): Record<string, string> {
  const out: Record<string, string> = {};
  if (city.trim()) out.city = city.trim();
  if (country.trim()) out.country = country.trim();
  return out;
}

// ---------------------------------------------------------------------------
// Add an event
// ---------------------------------------------------------------------------

function AddEventModal({
  shipmentId,
  onClose,
  onDone,
}: {
  shipmentId: string;
  onClose: () => void;
  onDone: () => void;
}) {
  const [eventType, setEventType] = useState<ShipmentEventType>("note");
  const [location, setLocation] = useState("");
  const [note, setNote] = useState("");

  const action = useAction(
    () => api.post(`ops/shipments/${shipmentId}/events`, { event_type: eventType, location, note }),
    { successMessage: "Event recorded", onSuccess: onDone },
  );
  const fe = action.fieldErrors;

  return (
    <Modal
      open
      onClose={onClose}
      title="Add a tracking event"
      description="Events are the customer-visible timeline. They do not change the shipment status on their own."
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" form="add-event" loading={action.pending} disabled={!note.trim() && !location.trim()}>
            Record event
          </Button>
        </>
      }
    >
      <form
        id="add-event"
        className="space-y-3"
        onSubmit={(e: FormEvent) => {
          e.preventDefault();
          void action.run();
        }}
      >
        <FormError message={action.error && !action.error.problem.errors?.length ? action.error.message : null} />
        <Select
          label="Event"
          options={shipmentEventOptions}
          value={eventType}
          onChange={(e) => setEventType(e.target.value as ShipmentEventType)}
          error={fe.event_type}
          required
        />
        <Input label="Location" value={location} onChange={(e) => setLocation(e.target.value)} error={fe.location} />
        <Textarea label="Note" rows={3} value={note} onChange={(e) => setNote(e.target.value)} error={fe.note} />
      </form>
    </Modal>
  );
}
