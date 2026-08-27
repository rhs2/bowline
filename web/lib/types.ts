/**
 * Types mirroring the Bowline API contract (docs/API.md) and the column names and
 * status sets in db/migrations. Money values are decimal strings ("1250.00") with a
 * separate currency field; timestamps are RFC 3339 UTC strings; dates are YYYY-MM-DD.
 */

export type Uuid = string;
export type Money = string;
export type IsoDateTime = string;
export type IsoDate = string;

// ---------------------------------------------------------------------------
// Envelopes and errors
// ---------------------------------------------------------------------------

export interface ListEnvelope<T> {
  items: T[];
  page: number;
  per_page: number;
  total: number;
}

export type ProblemCode =
  | "validation_failed"
  | "unauthorized"
  | "forbidden"
  | "not_found"
  | "conflict"
  | "invalid_transition"
  | "locked"
  | "rate_limited"
  | "internal";

export interface FieldError {
  field: string;
  message: string;
}

/** RFC 7807 problem document as emitted by the API. */
export interface Problem {
  type: string;
  title: string;
  status: number;
  detail?: string;
  code: ProblemCode | string;
  request_id?: string;
  errors?: FieldError[];
}

// ---------------------------------------------------------------------------
// Auth and principal
// ---------------------------------------------------------------------------

export interface TokenResponse {
  access_token: string;
  refresh_token: string;
  expires_in: number;
  must_change_password: boolean;
}

export interface MeUser {
  id: Uuid;
  email: string;
  must_change_password: boolean;
}

export interface ChainLink {
  id: Uuid;
  name: string;
  title: string;
  level: number;
}

export interface Me {
  user: MeUser;
  employee: Employee;
  roles: string[];
  permissions: string[];
  chain: ChainLink[];
}

// ---------------------------------------------------------------------------
// Organisation
// ---------------------------------------------------------------------------

export type EmployeeStatus = "active" | "on_leave" | "suspended" | "terminated";
export type EmploymentType = "full_time" | "part_time" | "contract";

export interface EmployeeRef {
  id: Uuid;
  name: string;
  title?: string | null;
}

export interface Employee {
  id: Uuid;
  employee_no: string;
  first_name: string;
  last_name: string;
  email: string;
  phone: string | null;
  position_id: Uuid;
  /** The position's title. The API calls this `title` on every employee route. */
  title: string;
  level: number;
  department_id: Uuid;
  department_name: string;
  manager_id: Uuid | null;
  status: EmployeeStatus;
  employment_type: EmploymentType;
  hire_date: IsoDate;
  termination_date: IsoDate | null;
  site: string | null;
  pay_grade?: string | null;
  base_salary?: Money;
  currency?: string;
  created_at?: IsoDateTime;
  updated_at?: IsoDateTime;
}

export interface EmployeeDetail extends Employee {
  manager: EmployeeRef | null;
  direct_reports_count: number;
}

export interface EmployeeCreateResponse extends EmployeeDetail {
  temporary_password: string;
}

export interface EmployeePatch {
  first_name?: string;
  last_name?: string;
  phone?: string | null;
  position_id?: Uuid;
  department_id?: Uuid;
  manager_id?: Uuid | null;
  status?: EmployeeStatus;
  employment_type?: EmploymentType;
  site?: string | null;
  pay_grade?: string | null;
  base_salary?: Money;
}

export interface OrgNode {
  id: Uuid;
  name: string;
  title: string;
  level: number;
  department: string;
  children: OrgNode[];
}

export interface Department {
  id: Uuid;
  code: string;
  name: string;
  parent_id: Uuid | null;
  cost_center: string | null;
  head: EmployeeRef | null;
  headcount: number;
}

export interface Position {
  id: Uuid;
  code: string;
  title: string;
  level: number;
  department_id: Uuid | null;
  is_people_manager: boolean;
}

// ---------------------------------------------------------------------------
// HR
// ---------------------------------------------------------------------------

export type LeaveStatus = "pending" | "approved" | "rejected" | "cancelled";

export interface LeaveType {
  key: string;
  name: string;
  paid: boolean;
  annual_quota_days: string | number;
}

export interface LeaveBalance {
  employee_id: Uuid;
  employee_name?: string;
  year: number;
  type_key: string;
  type_name?: string;
  allocated: string | number;
  used: string | number;
  /** allocated minus used, computed by the server. */
  remaining?: string | number;
}

export interface LeaveRequest {
  id: Uuid;
  employee_id: Uuid;
  employee_name?: string;
  type_key: string;
  start_date: IsoDate;
  end_date: IsoDate;
  days: string | number;
  reason: string | null;
  status: LeaveStatus;
  current_approver_id: Uuid | null;
  decided_by: Uuid | null;
  decided_at: IsoDateTime | null;
  decision_note: string | null;
  created_at: IsoDateTime;
}

export type ShiftStatus = "scheduled" | "completed" | "missed" | "cancelled";

export interface Shift {
  id: Uuid;
  employee_id: Uuid;
  employee_name?: string;
  site: string;
  starts_at: IsoDateTime;
  ends_at: IsoDateTime;
  role_on_shift: string | null;
  status: ShiftStatus;
  created_by: Uuid | null;
}

export type AttendanceSource = "web" | "mobile" | "kiosk" | "import";

export interface AttendanceRecord {
  id: Uuid;
  employee_id: Uuid;
  employee_name?: string;
  shift_id: Uuid | null;
  clock_in: IsoDateTime;
  clock_out: IsoDateTime | null;
  late: boolean;
  source: AttendanceSource;
}

export type EmployeeDocumentKind = "contract" | "id" | "certificate" | "payslip" | "other";

export interface EmployeeDocument {
  id: Uuid;
  employee_id: Uuid;
  kind: EmployeeDocumentKind;
  title: string;
  s3_key: string;
  mime_type: string;
  size_bytes: number;
  uploaded_by: Uuid | null;
  created_at: IsoDateTime;
}

export interface PresignResponse {
  upload_url: string;
  s3_key: string;
}

export interface DownloadResponse {
  url: string;
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------

export type TransportMode = "sea" | "air" | "road" | "rail";
export type Incoterm = "EXW" | "FCA" | "FOB" | "CIF" | "DAP" | "DDP";
export type CustomerStatus = "active" | "on_hold" | "closed";

export interface Address {
  line1?: string;
  line2?: string;
  city?: string;
  region?: string;
  postal_code?: string;
  country?: string;
}

export interface Customer {
  id: Uuid;
  code: string;
  name: string;
  contact_name: string | null;
  contact_email: string | null;
  phone: string | null;
  billing_address: Address;
  credit_limit: Money;
  currency: string;
  status: CustomerStatus;
  account_manager_id: Uuid | null;
  account_manager?: EmployeeRef | null;
  created_at: IsoDateTime;
}

export interface Carrier {
  id: Uuid;
  code: string;
  name: string;
  mode: TransportMode;
  scac: string | null;
  contact: Record<string, string>;
  on_time_rate: string | number | null;
  active: boolean;
}

export type SiteKind = "office" | "warehouse" | "port" | "airport" | "depot";

export interface Site {
  id: Uuid;
  code: string;
  name: string;
  kind: SiteKind;
  address: Address;
  manager_id: Uuid | null;
}

export type VehicleKind = "truck" | "van" | "trailer" | "forklift";
export type VehicleStatus = "available" | "in_use" | "maintenance" | "retired";

export interface Vehicle {
  id: Uuid;
  plate: string;
  kind: VehicleKind;
  capacity_kg: string | number | null;
  status: VehicleStatus;
  home_site_id: Uuid | null;
}

export type ShipmentStatus =
  | "draft"
  | "booked"
  | "picked_up"
  | "in_transit"
  | "customs"
  | "out_for_delivery"
  | "delivered"
  | "cancelled"
  | "exception";

export interface Location {
  city?: string;
  country?: string;
  port?: string;
}

export interface Shipment {
  id: Uuid;
  reference: string;
  customer_id: Uuid;
  customer_name?: string;
  mode: TransportMode;
  incoterm: Incoterm | null;
  origin: Location;
  destination: Location;
  cargo_description: string;
  pieces: number;
  weight_kg: string;
  volume_cbm: string | null;
  hazardous: boolean;
  declared_value: Money;
  currency: string;
  status: ShipmentStatus;
  previous_status: ShipmentStatus | null;
  etd: IsoDate | null;
  eta: IsoDate | null;
  delivered_at: IsoDateTime | null;
  delay_risk: string | number | null;
  owner_id: Uuid | null;
  owner_name?: string | null;
  created_by: Uuid | null;
  created_at: IsoDateTime;
  updated_at: IsoDateTime;
}

export type LegStatus = "planned" | "in_progress" | "completed" | "cancelled";

export interface ShipmentLeg {
  id: Uuid;
  shipment_id: Uuid;
  seq: number;
  mode: TransportMode;
  carrier_id: Uuid | null;
  carrier_name?: string | null;
  vehicle_id: Uuid | null;
  vehicle_plate?: string | null;
  driver_id: Uuid | null;
  driver_name?: string | null;
  from_location: Location;
  to_location: Location;
  planned_departure: IsoDateTime | null;
  planned_arrival: IsoDateTime | null;
  actual_departure: IsoDateTime | null;
  actual_arrival: IsoDateTime | null;
  status: LegStatus;
}

export type ShipmentEventType =
  | "created"
  | "booked"
  | "picked_up"
  | "departed"
  | "arrived"
  | "customs_hold"
  | "customs_cleared"
  | "out_for_delivery"
  | "delivered"
  | "exception"
  | "resumed"
  | "cancelled"
  | "note";

export interface ShipmentEvent {
  id: Uuid;
  shipment_id: Uuid;
  leg_id: Uuid | null;
  event_type: ShipmentEventType;
  occurred_at: IsoDateTime;
  location: string | null;
  note: string | null;
  recorded_by: Uuid | null;
  recorded_by_name?: string | null;
}

export type ShipmentDocumentKind =
  | "bill_of_lading"
  | "air_waybill"
  | "commercial_invoice"
  | "packing_list"
  | "customs"
  | "proof_of_delivery"
  | "other";

export interface ShipmentDocument {
  id: Uuid;
  shipment_id: Uuid;
  kind: ShipmentDocumentKind;
  title: string;
  s3_key: string;
  mime_type: string;
  size_bytes: number;
  uploaded_by: Uuid | null;
  created_at: IsoDateTime;
}

export type WorkOrderKind =
  | "loading"
  | "unloading"
  | "pickup"
  | "delivery"
  | "inspection"
  | "inventory";
export type WorkOrderStatus = "open" | "in_progress" | "done" | "blocked" | "cancelled";

export interface WorkOrder {
  id: Uuid;
  shipment_id: Uuid | null;
  shipment_reference?: string | null;
  site_id: Uuid | null;
  site_name?: string | null;
  kind: WorkOrderKind;
  title: string;
  instructions: string | null;
  assigned_to: Uuid | null;
  assigned_to_name?: string | null;
  assigned_by: Uuid | null;
  status: WorkOrderStatus;
  due_at: IsoDateTime | null;
  started_at: IsoDateTime | null;
  completed_at: IsoDateTime | null;
  notes: string | null;
  created_at: IsoDateTime;
}

export interface InventoryItem {
  id: Uuid;
  site_id: Uuid;
  shipment_id: Uuid | null;
  description: string;
  quantity: number;
  bin: string | null;
  received_at: IsoDateTime;
  released_at: IsoDateTime | null;
}

export interface ShipmentInvoiceSummary {
  id: Uuid;
  invoice_no: string;
  status: InvoiceStatus;
  total: Money;
  currency: string;
}

export interface ShipmentDetail extends Shipment {
  legs: ShipmentLeg[];
  events: ShipmentEvent[];
  documents: ShipmentDocument[];
  work_orders: WorkOrder[];
  invoice: ShipmentInvoiceSummary | null;
}

// ---------------------------------------------------------------------------
// Finance
// ---------------------------------------------------------------------------

export type AccountType = "asset" | "liability" | "equity" | "revenue" | "expense";

export interface Account {
  id: Uuid;
  code: string;
  name: string;
  type: AccountType;
  parent_id: Uuid | null;
  active: boolean;
}

export type PeriodStatus = "open" | "closed";

export interface FiscalPeriod {
  id: Uuid;
  year: number;
  month: number;
  starts_on: IsoDate;
  ends_on: IsoDate;
  status: PeriodStatus;
  closed_by: Uuid | null;
  closed_at: IsoDateTime | null;
}

export type JournalSource =
  | "invoice"
  | "payment"
  | "expense"
  | "payroll"
  | "bill"
  | "manual"
  | "reversal";

export interface JournalLine {
  id: Uuid;
  entry_id: Uuid;
  account_id: Uuid;
  account_code: string;
  account_name?: string;
  debit: Money;
  credit: Money;
  description: string | null;
}

export interface JournalEntry {
  id: Uuid;
  entry_no: number;
  period_id: Uuid;
  entry_date: IsoDate;
  memo: string;
  source_type: JournalSource;
  source_id: Uuid | null;
  posted_by: Uuid | null;
  posted_by_name?: string | null;
  posted_at: IsoDateTime;
  reverses_entry_id: Uuid | null;
  reversed_by_entry_id: Uuid | null;
  lines: JournalLine[];
}

export interface JournalLineInput {
  account_code: string;
  debit: Money;
  credit: Money;
  description: string;
}

export interface JournalEntryInput {
  entry_date: IsoDate;
  memo: string;
  lines: JournalLineInput[];
}

export type InvoiceStatus =
  | "draft"
  | "pending_approval"
  | "approved"
  | "issued"
  | "partially_paid"
  | "paid"
  | "void";

export interface InvoiceLine {
  id: Uuid;
  invoice_id: Uuid;
  seq: number;
  description: string;
  quantity: string;
  unit_price: Money;
  tax_rate: string;
  amount: Money;
}

export interface Invoice {
  id: Uuid;
  invoice_no: string;
  customer_id: Uuid;
  customer_name?: string;
  shipment_id: Uuid | null;
  shipment_reference?: string | null;
  status: InvoiceStatus;
  issue_date: IsoDate | null;
  due_date: IsoDate | null;
  currency: string;
  subtotal: Money;
  tax: Money;
  total: Money;
  amount_paid: Money;
  notes: string | null;
  pdf_s3_key: string | null;
  created_by: Uuid | null;
  approved_by: Uuid | null;
  issued_by: Uuid | null;
  journal_entry_id: Uuid | null;
  created_at: IsoDateTime;
  updated_at: IsoDateTime;
}

export type PaymentMethod = "bank_transfer" | "card" | "cash" | "cheque";

export interface Payment {
  id: Uuid;
  invoice_id: Uuid;
  received_on: IsoDate;
  amount: Money;
  method: PaymentMethod;
  reference: string | null;
  recorded_by: Uuid | null;
  journal_entry_id: Uuid | null;
  created_at: IsoDateTime;
}

export interface InvoiceDetail extends Invoice {
  lines: InvoiceLine[];
  payments: Payment[];
}

export interface InvoiceLineInput {
  description: string;
  quantity: string;
  unit_price: Money;
  tax_rate: string;
}

export interface InvoiceInput {
  customer_id: Uuid;
  shipment_id?: Uuid;
  currency: string;
  due_days: number;
  lines: InvoiceLineInput[];
}

export interface PaymentInput {
  invoice_id: Uuid;
  received_on: IsoDate;
  amount: Money;
  method: PaymentMethod;
  reference?: string;
}

export interface Vendor {
  id: Uuid;
  code: string;
  name: string;
  contact: Record<string, string>;
  active: boolean;
}

export type VendorBillStatus = "received" | "approved" | "paid" | "void";

export interface VendorBill {
  id: Uuid;
  vendor_id: Uuid;
  vendor_name?: string;
  bill_no: string;
  expense_account_id: Uuid;
  amount: Money;
  currency: string;
  received_on: IsoDate;
  due_on: IsoDate;
  status: VendorBillStatus;
  paid_on: IsoDate | null;
}

export type ExpenseCategory = "travel" | "fuel" | "meals" | "supplies" | "equipment" | "other";
export type ExpenseStatus =
  | "submitted"
  | "manager_approved"
  | "finance_approved"
  | "rejected"
  | "paid";

export interface Expense {
  id: Uuid;
  employee_id: Uuid;
  employee_name?: string;
  department_id: Uuid;
  category: ExpenseCategory;
  expense_account_id: Uuid;
  amount: Money;
  currency: string;
  incurred_on: IsoDate;
  description: string;
  receipt_s3_key: string | null;
  status: ExpenseStatus;
  manager_approved_by: Uuid | null;
  finance_approved_by: Uuid | null;
  rejected_by: Uuid | null;
  rejection_note: string | null;
  journal_entry_id: Uuid | null;
  created_at: IsoDateTime;
}

export interface ExpenseInput {
  category: ExpenseCategory;
  amount: Money;
  currency: string;
  incurred_on: IsoDate;
  description: string;
  receipt_s3_key?: string;
}

export type PayrollStatus = "draft" | "approved" | "posted";

export interface PayrollItem {
  id: Uuid;
  run_id: Uuid;
  employee_id: Uuid;
  employee_name?: string;
  gross: Money;
  deductions: Money;
  net: Money;
}

export interface PayrollRun {
  id: Uuid;
  period_id: Uuid;
  period?: { year: number; month: number } | null;
  status: PayrollStatus;
  total_gross: Money;
  total_deductions: Money;
  total_net: Money;
  created_by: Uuid | null;
  approved_by: Uuid | null;
  approved_at: IsoDateTime | null;
  posted_at: IsoDateTime | null;
  journal_entry_id: Uuid | null;
  created_at: IsoDateTime;
  items?: PayrollItem[];
}

export interface TrialBalanceRow {
  code: string;
  name: string;
  type: AccountType;
  debit: Money;
  credit: Money;
  balance: Money;
}

export interface TrialBalanceReport {
  rows: TrialBalanceRow[];
  total_debit: Money;
  total_credit: Money;
  /** The server's own verdict; debits and credits agree across the whole ledger. */
  balanced: boolean;
}

export type AgingBucket = "current" | "1-30" | "31-60" | "61-90" | "90+";

export interface ArAgingRow {
  invoice_id: Uuid;
  invoice_no: string;
  customer_id: Uuid;
  customer_name: string;
  due_date: IsoDate;
  outstanding: Money;
  days_overdue: number;
  bucket: AgingBucket;
}

/** One aging bucket's rollup, as returned in `ArAgingReport.buckets`. */
export interface ArAgingBucketTotal {
  bucket: AgingBucket;
  invoices: number;
  outstanding: Money;
}

export interface ArAgingReport {
  as_of: IsoDate;
  rows: ArAgingRow[];
  buckets: ArAgingBucketTotal[];
  total_outstanding: Money;
  /** The part of `total_outstanding` that is past its due date. */
  total_overdue: Money;
}

export interface PnlLine {
  code: string;
  name: string;
  type: "revenue" | "expense";
  amount: Money;
}

export interface PnlReport {
  year: number;
  month: number | null;
  revenue: PnlLine[];
  expenses: PnlLine[];
  total_revenue: Money;
  total_expenses: Money;
  net_income: Money;
}

// ---------------------------------------------------------------------------
// Communications and support
// ---------------------------------------------------------------------------

export type ThreadKind = "direct" | "announcement" | "ticket";
export type Importance = "low" | "normal" | "high";
export type AnnouncementScope = "company" | "department" | "subtree";

export interface Recipient {
  id: Uuid;
  name: string;
  title: string;
  department: string;
  email?: string;
}

export interface Message {
  id: Uuid;
  thread_id: Uuid;
  sender_id: Uuid | null;
  sender_name?: string | null;
  body: string;
  importance: Importance;
  sent_at: IsoDateTime;
}

export interface ThreadParticipant {
  employee_id: Uuid;
  name: string;
  role: "sender" | "recipient" | "cc" | "agent";
  last_read_at: IsoDateTime | null;
}

export interface Thread {
  id: Uuid;
  kind: ThreadKind;
  subject: string;
  created_by: Uuid | null;
  created_by_name?: string | null;
  audience: { scope: AnnouncementScope; ref?: Uuid | null } | null;
  created_at: IsoDateTime;
  last_message_at: IsoDateTime;
  unread_count: number;
  last_message: Pick<Message, "body" | "sender_id" | "sender_name" | "sent_at"> | null;
}

export interface ThreadDetail extends Thread {
  messages: Message[];
  participants: ThreadParticipant[];
}

export type TicketCategory = "it" | "hr" | "payroll" | "operations" | "facilities" | "other";
export type TicketPriority = "low" | "normal" | "high" | "urgent";
export type TicketStatus =
  | "open"
  | "triaged"
  | "in_progress"
  | "waiting_on_requester"
  | "resolved"
  | "closed";

export interface Ticket {
  id: Uuid;
  ticket_no: string;
  thread_id: Uuid;
  subject: string;
  requester_id: Uuid;
  requester_name?: string;
  category: TicketCategory;
  priority: TicketPriority;
  status: TicketStatus;
  assignee_id: Uuid | null;
  assignee_name?: string | null;
  sla_due_at: IsoDateTime;
  first_response_at: IsoDateTime | null;
  resolved_at: IsoDateTime | null;
  closed_at: IsoDateTime | null;
  satisfaction: number | null;
  created_at: IsoDateTime;
  updated_at: IsoDateTime;
}

export interface TicketDetail extends Ticket {
  messages: Message[];
}

// ---------------------------------------------------------------------------
// Admin and platform
// ---------------------------------------------------------------------------

export type UserStatus = "active" | "locked" | "disabled";

export interface AdminUser {
  id: Uuid;
  employee_id: Uuid;
  employee_name?: string;
  email: string;
  status: UserStatus;
  roles: string[];
  must_change_password: boolean;
  failed_logins: number;
  locked_until: IsoDateTime | null;
  last_login_at: IsoDateTime | null;
  created_at: IsoDateTime;
}

export interface ResetPasswordResponse {
  temporary_password: string;
}

export interface Role {
  id: number;
  key: string;
  name: string;
  description: string;
  permissions: string[];
}

export interface AuditEntry {
  id: number;
  at: IsoDateTime;
  actor_user_id: Uuid | null;
  actor_employee_id: Uuid | null;
  actor_name?: string | null;
  action: string;
  entity_type: string;
  entity_id: Uuid | null;
  before: unknown;
  after: unknown;
  ip: string | null;
  request_id: string | null;
}

/**
 * Role-aware summary from GET /dashboard. Every counter is optional: the API only
 * includes the ones the caller is entitled to (AR outstanding for finance, headcount
 * for HR and executives), and the UI renders a card per value that is present.
 */
/**
 * The role-aware summary from `GET /dashboard`.
 *
 * The personal blocks are always present. The rest appear only when the caller
 * holds the permission behind them, so every one of those is optional.
 */
export interface Dashboard {
  employee_id: Uuid;
  name: string;
  title: string;
  roles: string[];
  generated_at: IsoDateTime;

  my_work: { open: number; overdue: number; next: DashboardTask[] };
  my_leave: { pending: number; upcoming_approved: number; next_start: IsoDate | null };
  my_messages: { unread: number; threads: number };
  my_tickets: { open: number; assigned_to_me: number; awaiting_my_close: number };

  /** Approvers only. Absent for anyone who approves nothing. */
  waiting_on_me?: { leave_requests: number; expense_claims: number };
  shipments?: { total: number; by_status: Array<{ status: ShipmentStatus; count: number }> };
  receivables?: { outstanding: Money; overdue: Money; open_invoices: number; overdue_invoices: number };
  people?: { headcount: number; on_leave_today: number; joined_this_month: number };
  service_desk?: { open: number; unassigned: number; breaching_sla: number };
}

export interface DashboardTask {
  id: Uuid;
  title: string;
  kind: string;
  status: string;
  due_at: IsoDateTime | null;
  shipment_reference?: string | null;
}
