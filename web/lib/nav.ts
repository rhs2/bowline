import { can, canAny, canAnyOf, canSeeOthers, has } from "./permissions";

export interface NavItem {
  href: string;
  label: string;
  /** Visible when this returns true for the principal's permission set. */
  visible: (permissions: readonly string[]) => boolean;
}

export interface NavSection {
  label: string;
  items: NavItem[];
}

const always = () => true;

/**
 * Sidebar navigation filtered by permission. The gates mirror the permission column
 * of docs/API.md so a link is only shown when the page behind it can load.
 */
export const NAV_SECTIONS: NavSection[] = [
  {
    label: "Workspace",
    items: [
      { href: "/dashboard", label: "Dashboard", visible: always },
      { href: "/inbox", label: "Inbox", visible: always },
      { href: "/announcements", label: "Announcements", visible: always },
      { href: "/support", label: "Support", visible: always },
    ],
  },
  {
    label: "Organisation",
    items: [
      { href: "/org", label: "Org chart", visible: (p) => has(p, "org:read") },
      { href: "/people", label: "People", visible: (p) => canSeeOthers(p) },
    ],
  },
  {
    label: "HR",
    items: [
      { href: "/hr/leave", label: "Leave", visible: (p) => canAny(p, "leave") },
      { href: "/hr/shifts", label: "Shifts", visible: always },
      { href: "/hr/attendance", label: "Attendance", visible: (p) => has(p, "attendance:record:self") },
      { href: "/hr/documents", label: "Documents", visible: (p) => canAny(p, "documents") },
    ],
  },
  {
    label: "Operations",
    items: [
      { href: "/ops/work-orders", label: "Work orders", visible: (p) => canAny(p, "tasks") },
      { href: "/ops/shipments", label: "Shipments", visible: (p) => has(p, "shipments:read") },
      { href: "/ops/customers", label: "Customers", visible: (p) => canAny(p, "customers") },
      {
        href: "/ops/fleet",
        label: "Fleet",
        visible: (p) => canAnyOf(p, ["fleet:manage", "shipments:write", "shipments:assign"]),
      },
    ],
  },
  {
    label: "Finance",
    items: [
      { href: "/finance/expenses", label: "Expenses", visible: (p) => canAny(p, "expenses") },
      {
        href: "/finance/invoices",
        label: "Invoices",
        visible: (p) => canAnyOf(p, ["ledger:read", "customers:read"]),
      },
      { href: "/finance/ledger", label: "Ledger", visible: (p) => has(p, "ledger:read") },
      { href: "/finance/payroll", label: "Payroll", visible: (p) => canAny(p, "payroll") },
      { href: "/finance/reports", label: "Reports", visible: (p) => has(p, "ledger:read") },
    ],
  },
  {
    label: "Administration",
    items: [
      { href: "/admin/users", label: "Users", visible: (p) => has(p, "users:manage") },
      { href: "/admin/roles", label: "Roles", visible: (p) => has(p, "roles:manage") },
      { href: "/admin/audit", label: "Audit log", visible: (p) => can(p, "audit:read") },
    ],
  },
];

export function visibleNav(permissions: readonly string[]): NavSection[] {
  return NAV_SECTIONS.map((section) => ({
    label: section.label,
    items: section.items.filter((item) => item.visible(permissions)),
  })).filter((section) => section.items.length > 0);
}
