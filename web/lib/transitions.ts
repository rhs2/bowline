import type {
  ExpenseStatus,
  InvoiceStatus,
  PayrollStatus,
  ShipmentStatus,
  TicketStatus,
  WorkOrderStatus,
} from "./types";
import { can, has } from "./permissions";

// ---------------------------------------------------------------------------
// Shipments (docs/DOMAIN.md, "Shipment state machine")
//
//   draft -> booked -> picked_up -> in_transit -> customs -> out_for_delivery -> delivered
//   any non-terminal state -> exception -> (back to the previous state) | cancelled
//   draft | booked -> cancelled
// ---------------------------------------------------------------------------

export const SHIPMENT_FLOW: readonly ShipmentStatus[] = [
  "draft",
  "booked",
  "picked_up",
  "in_transit",
  "customs",
  "out_for_delivery",
  "delivered",
];

export const SHIPMENT_TERMINAL: readonly ShipmentStatus[] = ["delivered", "cancelled"];

function nextInFlow(status: ShipmentStatus): ShipmentStatus | null {
  const idx = SHIPMENT_FLOW.indexOf(status);
  if (idx < 0) return null;
  return SHIPMENT_FLOW[idx + 1] ?? null;
}

/**
 * Legal next states for a shipment. `previous` is `shipments.previous_status`, used
 * to resume from `exception`.
 */
export function shipmentTransitions(
  status: ShipmentStatus,
  previous: ShipmentStatus | null = null,
): ShipmentStatus[] {
  if (status === "exception") {
    const out: ShipmentStatus[] = [];
    if (previous && previous !== "exception" && !SHIPMENT_TERMINAL.includes(previous)) {
      out.push(previous);
    }
    out.push("cancelled");
    return out;
  }
  if (SHIPMENT_TERMINAL.includes(status)) return [];
  const out: ShipmentStatus[] = [];
  const next = nextInFlow(status);
  if (next) out.push(next);
  if (status === "draft" || status === "booked") out.push("cancelled");
  out.push("exception");
  return out;
}

export function canTransitionShipment(
  from: ShipmentStatus,
  to: ShipmentStatus,
  previous: ShipmentStatus | null = null,
): boolean {
  return shipmentTransitions(from, previous).includes(to);
}

// ---------------------------------------------------------------------------
// Support tickets
//   open -> triaged -> in_progress -> waiting_on_requester -> resolved -> closed
//   requester may close, and may reopen a resolved ticket within 7 days
// ---------------------------------------------------------------------------

const AGENT_TICKET_NEXT: Record<TicketStatus, TicketStatus[]> = {
  open: ["triaged", "in_progress", "resolved"],
  triaged: ["in_progress", "waiting_on_requester", "resolved"],
  in_progress: ["waiting_on_requester", "resolved"],
  waiting_on_requester: ["in_progress", "resolved"],
  resolved: ["closed", "in_progress"],
  closed: [],
};

export const REOPEN_WINDOW_DAYS = 7;

export function ticketTransitions(
  status: TicketStatus,
  opts: { isAgent: boolean; isRequester: boolean; resolvedAt?: string | null; now?: Date },
): TicketStatus[] {
  const out = new Set<TicketStatus>();
  if (opts.isAgent) {
    for (const s of AGENT_TICKET_NEXT[status]) out.add(s);
  }
  if (opts.isRequester) {
    if (status !== "closed" && status !== "resolved") out.add("closed");
    if (status === "resolved") {
      out.add("closed");
      const now = opts.now ?? new Date();
      const resolvedAt = opts.resolvedAt ? new Date(opts.resolvedAt) : null;
      const withinWindow =
        !resolvedAt ||
        now.getTime() - resolvedAt.getTime() <= REOPEN_WINDOW_DAYS * 24 * 60 * 60 * 1000;
      if (withinWindow) out.add("open");
    }
  }
  return [...out];
}

// ---------------------------------------------------------------------------
// Invoices
//   draft -> pending_approval | approved -> issued -> partially_paid -> paid
//   void from draft, approved or issued
// ---------------------------------------------------------------------------

export type InvoiceAction = "submit" | "approve" | "issue" | "void" | "record_payment";

export function invoiceActions(status: InvoiceStatus, permissions: readonly string[]): InvoiceAction[] {
  const out: InvoiceAction[] = [];
  const draft = has(permissions, "invoices:draft");
  const approve = has(permissions, "invoices:approve");
  const issue = has(permissions, "invoices:issue");
  const pay = has(permissions, "payments:record");
  switch (status) {
    case "draft":
      if (draft) out.push("submit");
      if (approve) out.push("void");
      break;
    case "pending_approval":
      if (approve) out.push("approve", "void");
      break;
    case "approved":
      if (issue) out.push("issue");
      if (approve) out.push("void");
      break;
    case "issued":
    case "partially_paid":
      if (pay) out.push("record_payment");
      if (approve && status === "issued") out.push("void");
      break;
    case "paid":
    case "void":
      break;
  }
  return out;
}

// ---------------------------------------------------------------------------
// Expense claims
//   submitted -> manager_approved -> finance_approved -> paid; rejected before paid
// ---------------------------------------------------------------------------

export type ExpenseAction = "approve" | "reject" | "pay";

/** The step a claim is waiting on, or null once it is settled. */
export type ExpenseStep = "manager" | "finance" | "payment" | null;

export function expensePendingStep(status: ExpenseStatus): ExpenseStep {
  switch (status) {
    case "submitted":
      return "manager";
    case "manager_approved":
      return "finance";
    case "finance_approved":
      return "payment";
    case "rejected":
    case "paid":
      return null;
  }
}

/** Plain-language label for the queue: who is holding the claim up. */
export function expenseStepLabel(status: ExpenseStatus): string {
  switch (expensePendingStep(status)) {
    case "manager":
      return "Waiting on the manager";
    case "finance":
      return "Waiting on finance";
    case "payment":
      return "Approved, waiting to be paid";
    default:
      return status === "paid" ? "Paid" : "Rejected";
  }
}

/** True when the caller's permissions let them act on the claim's current step. */
export function isMyExpenseStep(status: ExpenseStatus, permissions: readonly string[]): boolean {
  const step = expensePendingStep(status);
  if (step === null) return false;
  if (step === "manager") {
    return can(permissions, "expenses:approve:subtree") || has(permissions, "expenses:approve:finance");
  }
  return has(permissions, "expenses:approve:finance");
}

export function expenseActions(status: ExpenseStatus, permissions: readonly string[]): ExpenseAction[] {
  const manager = can(permissions, "expenses:approve:subtree");
  const finance = has(permissions, "expenses:approve:finance");
  switch (status) {
    case "submitted":
      return manager || finance ? ["approve", "reject"] : [];
    case "manager_approved":
      return finance ? ["approve", "reject"] : [];
    case "finance_approved":
      return finance ? ["pay", "reject"] : [];
    case "rejected":
    case "paid":
      return [];
  }
}

// ---------------------------------------------------------------------------
// Work orders and payroll
// ---------------------------------------------------------------------------

export function workOrderTransitions(status: WorkOrderStatus): WorkOrderStatus[] {
  switch (status) {
    case "open":
      return ["in_progress", "blocked"];
    case "in_progress":
      return ["done", "blocked"];
    case "blocked":
      return ["in_progress"];
    case "done":
    case "cancelled":
      return [];
  }
}

export type PayrollAction = "approve" | "post";

export function payrollActions(status: PayrollStatus, permissions: readonly string[]): PayrollAction[] {
  if (!has(permissions, "payroll:approve")) return [];
  if (status === "draft") return ["approve"];
  if (status === "approved") return ["post"];
  return [];
}
