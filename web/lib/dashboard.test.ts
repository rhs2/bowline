import { describe, expect, it } from "vitest";
import { statCards } from "./dashboard";
import type { Dashboard } from "./types";

/**
 * Intl.NumberFormat separates the currency from the amount with a non-breaking
 * space, so compare on normalised whitespace rather than pinning that character.
 */
const spaces = (v: string | undefined) => (v ?? "").replace(/\s+/g, " ");

/**
 * The personal blocks the API always sends. The wider blocks are permission
 * gated, so they are added per test rather than assumed here.
 */
function base(): Dashboard {
  return {
    employee_id: "11111111-1111-1111-1111-111111111111",
    name: "Nina Reinders",
    title: "Dock Worker",
    roles: ["baseline", "field_worker"],
    generated_at: "2026-08-27T08:00:00Z",
    my_work: { open: 0, overdue: 0, next: [] },
    my_leave: { pending: 0, upcoming_approved: 0, next_start: null },
    my_messages: { unread: 0, threads: 0 },
    my_tickets: { open: 0, assigned_to_me: 0, awaiting_my_close: 0 },
  };
}

describe("statCards", () => {
  it("renders nothing for a quiet field worker", () => {
    expect(statCards(base())).toEqual([]);
  });

  it("does not throw when the permission gated blocks are absent", () => {
    // A field worker approves nothing and sees no finance, so the API omits
    // waiting_on_me, receivables, people and service_desk entirely. Reading them
    // without a guard is what broke this page before.
    const d = base();
    expect(d.waiting_on_me).toBeUndefined();
    expect(() => statCards(d)).not.toThrow();
  });

  it("counts leave and expense approvals together", () => {
    const d = { ...base(), waiting_on_me: { leave_requests: 2, expense_claims: 15 } };
    const card = statCards(d).find((c) => c.key === "waiting_on_me");
    expect(card?.value).toBe("17");
    expect(card?.hint).toBe("2 leave, 15 expenses");
  });

  it("hides the approvals card when the queue is empty", () => {
    const d = { ...base(), waiting_on_me: { leave_requests: 0, expense_claims: 0 } };
    expect(statCards(d).find((c) => c.key === "waiting_on_me")).toBeUndefined();
  });

  it("formats receivables as money and names the overdue part", () => {
    const d: Dashboard = {
      ...base(),
      receivables: {
        outstanding: "2739610.51",
        overdue: "1378494.63",
        open_invoices: 42,
        overdue_invoices: 24,
      },
    };
    const card = statCards(d).find((c) => c.key === "receivable");
    expect(spaces(card?.value)).toBe("USD 2,739,610.51");
    expect(spaces(card?.hint)).toContain("USD 1,378,494.63 overdue across 24 invoices");
  });

  it("shows the wider blocks only when the API sends them", () => {
    const quiet = statCards(base()).map((c) => c.key);
    expect(quiet).not.toContain("people");
    expect(quiet).not.toContain("service_desk");
    expect(quiet).not.toContain("shipments");

    const exec: Dashboard = {
      ...base(),
      shipments: { total: 128, by_status: [{ status: "in_transit", count: 38 }] },
      people: { headcount: 254, on_leave_today: 0, joined_this_month: 4 },
      service_desk: { open: 35, unassigned: 8, breaching_sla: 8 },
    };
    const keys = statCards(exec).map((c) => c.key);
    expect(keys).toContain("shipments");
    expect(keys).toContain("people");
    expect(keys).toContain("service_desk");
  });

  it("surfaces overdue work in the hint", () => {
    const d = { ...base(), my_work: { open: 3, overdue: 1, next: [] } };
    const card = statCards(d).find((c) => c.key === "my_work");
    expect(card?.value).toBe("3");
    expect(card?.hint).toBe("1 overdue");
  });

  it("every card carries a link so the number is actionable", () => {
    const d: Dashboard = {
      ...base(),
      my_work: { open: 1, overdue: 0, next: [] },
      my_messages: { unread: 4, threads: 2 },
      waiting_on_me: { leave_requests: 1, expense_claims: 0 },
      people: { headcount: 254, on_leave_today: 1, joined_this_month: 2 },
    };
    const cards = statCards(d);
    expect(cards.length).toBeGreaterThan(0);
    for (const c of cards) {
      expect(c.href).toMatch(/^\//);
      expect(c.label).not.toBe("");
      expect(c.value).not.toBe("undefined");
    }
  });
});
