import { describe, expect, it } from "vitest";
import { NAV_SECTIONS, visibleNav } from "./nav";

/** Baseline permissions every active user holds (docs/DOMAIN.md). */
const BASELINE = [
  "org:read",
  "employees:read:self",
  "leave:request",
  "attendance:record:self",
  "documents:read:self",
  "tasks:read:self",
  "tasks:update:self",
  "expenses:submit",
  "messages:send:chain",
  "messages:send:department",
  "tickets:create",
  "shipments:read",
];

const ACCOUNTANT = [
  ...BASELINE,
  "ledger:read",
  "ledger:post",
  "invoices:draft",
  "invoices:issue",
  "payments:record",
  "vendors:manage",
  "payroll:prepare",
  "reports:read:all",
];

const IT_ADMIN = [...BASELINE, "users:manage", "roles:manage", "audit:read", "system:admin"];

const DISPATCHER = [
  ...BASELINE,
  "shipments:write",
  "shipments:assign",
  "fleet:manage",
  "tasks:manage:subtree",
  "customers:read",
];

function hrefs(permissions: readonly string[]): string[] {
  return visibleNav(permissions).flatMap((section) => section.items.map((item) => item.href));
}

describe("visibleNav", () => {
  it("shows a field worker only their own workspace, HR and operations reading", () => {
    const visible = hrefs(BASELINE);
    expect(visible).toContain("/dashboard");
    expect(visible).toContain("/hr/leave");
    expect(visible).toContain("/hr/shifts");
    expect(visible).toContain("/hr/attendance");
    expect(visible).toContain("/hr/documents");
    expect(visible).toContain("/ops/work-orders");
    expect(visible).toContain("/ops/shipments");
    expect(visible).toContain("/finance/expenses");
  });

  it("hides finance and administration from a field worker", () => {
    const visible = hrefs(BASELINE);
    expect(visible).not.toContain("/finance/invoices");
    expect(visible).not.toContain("/finance/ledger");
    expect(visible).not.toContain("/finance/payroll");
    expect(visible).not.toContain("/finance/reports");
    expect(visible).not.toContain("/admin/users");
    expect(visible).not.toContain("/admin/roles");
    expect(visible).not.toContain("/admin/audit");
    expect(visible).not.toContain("/people");
  });

  it("opens the finance section to an accountant", () => {
    const visible = hrefs(ACCOUNTANT);
    expect(visible).toContain("/finance/invoices");
    expect(visible).toContain("/finance/ledger");
    expect(visible).toContain("/finance/payroll");
    expect(visible).toContain("/finance/reports");
    expect(visible).not.toContain("/admin/users");
  });

  it("opens the administration section to an IT admin", () => {
    const visible = hrefs(IT_ADMIN);
    expect(visible).toContain("/admin/users");
    expect(visible).toContain("/admin/roles");
    expect(visible).toContain("/admin/audit");
  });

  it("gives a dispatcher customers and fleet", () => {
    const visible = hrefs(DISPATCHER);
    expect(visible).toContain("/ops/customers");
    expect(visible).toContain("/ops/fleet");
  });

  it("drops sections that end up empty", () => {
    const sections = visibleNav(BASELINE).map((s) => s.label);
    expect(sections).not.toContain("Administration");
    expect(sections.every((label) => visibleNav(BASELINE).find((s) => s.label === label)!.items.length > 0)).toBe(true);
  });

  it("shows nothing at all to a principal with no permissions except the always-on items", () => {
    const visible = hrefs([]);
    expect(visible).toEqual(["/dashboard", "/inbox", "/announcements", "/support", "/hr/shifts"]);
  });

  it("keeps every declared route reachable by some permission set", () => {
    const everyHref = NAV_SECTIONS.flatMap((s) => s.items.map((i) => i.href));
    const all = new Set([...hrefs(ACCOUNTANT), ...hrefs(IT_ADMIN), ...hrefs(DISPATCHER), ...hrefs(BASELINE)]);
    for (const href of everyHref) {
      expect(all.has(href) || href === "/people" || href === "/org").toBe(true);
    }
  });
});
