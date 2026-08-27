import type { SelectOption } from "@/components/ui/Field";
import { humanize } from "./format";

function opts<T extends string>(values: readonly T[]): Array<SelectOption & { value: T }> {
  return values.map((v) => ({ value: v, label: humanize(v) }));
}

export const EMPLOYEE_STATUSES = ["active", "on_leave", "suspended", "terminated"] as const;
export const EMPLOYMENT_TYPES = ["full_time", "part_time", "contract"] as const;
export const LEAVE_STATUSES = ["pending", "approved", "rejected", "cancelled"] as const;
export const DOCUMENT_KINDS = ["contract", "id", "certificate", "payslip", "other"] as const;
export const SHIPMENT_DOCUMENT_KINDS = [
  "bill_of_lading",
  "air_waybill",
  "commercial_invoice",
  "packing_list",
  "customs",
  "proof_of_delivery",
  "other",
] as const;
export const TRANSPORT_MODES = ["sea", "air", "road", "rail"] as const;
export const INCOTERMS = ["EXW", "FCA", "FOB", "CIF", "DAP", "DDP"] as const;
export const SHIPMENT_STATUSES = [
  "draft",
  "booked",
  "picked_up",
  "in_transit",
  "customs",
  "out_for_delivery",
  "delivered",
  "exception",
  "cancelled",
] as const;
export const SHIPMENT_EVENT_TYPES = [
  "note",
  "picked_up",
  "departed",
  "arrived",
  "customs_hold",
  "customs_cleared",
  "out_for_delivery",
  "delivered",
  "exception",
] as const;
export const WORK_ORDER_KINDS = ["loading", "unloading", "pickup", "delivery", "inspection", "inventory"] as const;
export const WORK_ORDER_STATUSES = ["open", "in_progress", "done", "blocked", "cancelled"] as const;
export const CUSTOMER_STATUSES = ["active", "on_hold", "closed"] as const;
export const SITE_KINDS = ["office", "warehouse", "port", "airport", "depot"] as const;
export const VEHICLE_KINDS = ["truck", "van", "trailer", "forklift"] as const;
export const VEHICLE_STATUSES = ["available", "in_use", "maintenance", "retired"] as const;
export const INVOICE_STATUSES = [
  "draft",
  "pending_approval",
  "approved",
  "issued",
  "partially_paid",
  "paid",
  "void",
] as const;
export const PAYMENT_METHODS = ["bank_transfer", "card", "cash", "cheque"] as const;
export const EXPENSE_CATEGORIES = ["travel", "fuel", "meals", "supplies", "equipment", "other"] as const;
export const EXPENSE_STATUSES = ["submitted", "manager_approved", "finance_approved", "rejected", "paid"] as const;
export const TICKET_CATEGORIES = ["it", "hr", "payroll", "operations", "facilities", "other"] as const;
export const TICKET_PRIORITIES = ["low", "normal", "high", "urgent"] as const;
export const TICKET_STATUSES = [
  "open",
  "triaged",
  "in_progress",
  "waiting_on_requester",
  "resolved",
  "closed",
] as const;
export const IMPORTANCE = ["low", "normal", "high"] as const;
export const LEVELS = [1, 2, 3, 4, 5, 6, 7] as const;

export const employeeStatusOptions = opts(EMPLOYEE_STATUSES);
export const employmentTypeOptions = opts(EMPLOYMENT_TYPES);
export const leaveStatusOptions = opts(LEAVE_STATUSES);
export const documentKindOptions = opts(DOCUMENT_KINDS).map((o) =>
  o.value === "id" ? { ...o, label: "ID document" } : o,
);
export const shipmentDocumentKindOptions = opts(SHIPMENT_DOCUMENT_KINDS);
export const modeOptions = opts(TRANSPORT_MODES);
export const incotermOptions = INCOTERMS.map((v) => ({ value: v, label: v }));
export const shipmentStatusOptions = opts(SHIPMENT_STATUSES);
export const shipmentEventOptions = opts(SHIPMENT_EVENT_TYPES);
export const workOrderKindOptions = opts(WORK_ORDER_KINDS);
export const workOrderStatusOptions = opts(WORK_ORDER_STATUSES);
export const customerStatusOptions = opts(CUSTOMER_STATUSES);
export const siteKindOptions = opts(SITE_KINDS);
export const vehicleKindOptions = opts(VEHICLE_KINDS);
export const vehicleStatusOptions = opts(VEHICLE_STATUSES);
export const invoiceStatusOptions = opts(INVOICE_STATUSES);
export const paymentMethodOptions = opts(PAYMENT_METHODS);
export const expenseCategoryOptions = opts(EXPENSE_CATEGORIES);
export const expenseStatusOptions = opts(EXPENSE_STATUSES);
export const ticketCategoryOptions = opts(TICKET_CATEGORIES).map((o) =>
  o.value === "it" ? { ...o, label: "IT" } : o.value === "hr" ? { ...o, label: "HR" } : o,
);
export const ticketPriorityOptions = opts(TICKET_PRIORITIES);
export const ticketStatusOptions = opts(TICKET_STATUSES);
export const importanceOptions = opts(IMPORTANCE);
export const levelOptions: SelectOption[] = [
  { value: "1", label: "1, Chief" },
  { value: "2", label: "2, C-suite" },
  { value: "3", label: "3, Director" },
  { value: "4", label: "4, Manager" },
  { value: "5", label: "5, Supervisor" },
  { value: "6", label: "6, Specialist" },
  { value: "7", label: "7, Ground" },
];

/** SLA time to first response by priority, in hours (docs/DOMAIN.md). */
export const SLA_HOURS: Record<string, number> = { urgent: 1, high: 4, normal: 24, low: 72 };
