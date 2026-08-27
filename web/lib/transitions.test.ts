import { describe, expect, it } from "vitest";
import {
  canTransitionShipment,
  expenseActions,
  expensePendingStep,
  expenseStepLabel,
  invoiceActions,
  isMyExpenseStep,
  payrollActions,
  shipmentTransitions,
  ticketTransitions,
  workOrderTransitions,
  SHIPMENT_FLOW,
} from "./transitions";
import type { ShipmentStatus } from "./types";

// ---------------------------------------------------------------------------
// Shipments: docs/DOMAIN.md, "Shipment state machine"
//
//   draft -> booked -> picked_up -> in_transit -> customs -> out_for_delivery -> delivered
//   any non-terminal state -> exception -> (back to the previous state) | cancelled
//   draft | booked -> cancelled
// ---------------------------------------------------------------------------

describe("shipmentTransitions", () => {
  it("offers the next step in the happy path from every non-terminal state", () => {
    for (let i = 0; i < SHIPMENT_FLOW.length - 1; i += 1) {
      const from = SHIPMENT_FLOW[i] as ShipmentStatus;
      const next = SHIPMENT_FLOW[i + 1] as ShipmentStatus;
      expect(shipmentTransitions(from)).toContain(next);
    }
  });

  it("never skips a step in the happy path", () => {
    expect(shipmentTransitions("draft")).not.toContain("picked_up");
    expect(shipmentTransitions("booked")).not.toContain("in_transit");
    expect(shipmentTransitions("in_transit")).not.toContain("delivered");
  });

  it("allows cancellation only from draft and booked", () => {
    expect(shipmentTransitions("draft")).toContain("cancelled");
    expect(shipmentTransitions("booked")).toContain("cancelled");
    for (const status of ["picked_up", "in_transit", "customs", "out_for_delivery"] as ShipmentStatus[]) {
      expect(shipmentTransitions(status)).not.toContain("cancelled");
    }
  });

  it("offers an exception from every non-terminal state", () => {
    for (const status of SHIPMENT_FLOW.slice(0, -1) as ShipmentStatus[]) {
      expect(shipmentTransitions(status)).toContain("exception");
    }
  });

  it("returns nothing from the terminal states", () => {
    expect(shipmentTransitions("delivered")).toEqual([]);
    expect(shipmentTransitions("cancelled")).toEqual([]);
  });

  it("resumes an exception back into the previous state, or cancels", () => {
    expect(shipmentTransitions("exception", "in_transit")).toEqual(["in_transit", "cancelled"]);
    expect(shipmentTransitions("exception", "customs")).toEqual(["customs", "cancelled"]);
  });

  it("only offers cancellation when the previous state is unknown or terminal", () => {
    expect(shipmentTransitions("exception", null)).toEqual(["cancelled"]);
    expect(shipmentTransitions("exception", "delivered")).toEqual(["cancelled"]);
    expect(shipmentTransitions("exception", "exception")).toEqual(["cancelled"]);
  });

  it("does not offer a second exception while already in exception", () => {
    expect(shipmentTransitions("exception", "booked")).not.toContain("exception");
  });
});

describe("canTransitionShipment", () => {
  it("agrees with the list of legal next states", () => {
    expect(canTransitionShipment("draft", "booked")).toBe(true);
    expect(canTransitionShipment("draft", "delivered")).toBe(false);
    expect(canTransitionShipment("delivered", "exception")).toBe(false);
    expect(canTransitionShipment("exception", "customs", "customs")).toBe(true);
    expect(canTransitionShipment("exception", "customs", "booked")).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// Expense claims: submitted -> manager_approved -> finance_approved -> paid
// ---------------------------------------------------------------------------

describe("expensePendingStep", () => {
  it("names the step a claim is waiting on", () => {
    expect(expensePendingStep("submitted")).toBe("manager");
    expect(expensePendingStep("manager_approved")).toBe("finance");
    expect(expensePendingStep("finance_approved")).toBe("payment");
  });

  it("returns null once the claim is settled", () => {
    expect(expensePendingStep("paid")).toBeNull();
    expect(expensePendingStep("rejected")).toBeNull();
  });
});

describe("expenseStepLabel", () => {
  it("reads as plain language for the approval queue", () => {
    expect(expenseStepLabel("submitted")).toBe("Waiting on the manager");
    expect(expenseStepLabel("manager_approved")).toBe("Waiting on finance");
    expect(expenseStepLabel("finance_approved")).toBe("Approved, waiting to be paid");
    expect(expenseStepLabel("paid")).toBe("Paid");
    expect(expenseStepLabel("rejected")).toBe("Rejected");
  });
});

describe("isMyExpenseStep", () => {
  const manager = ["expenses:approve:subtree"];
  const finance = ["expenses:approve:finance"];
  const nobody = ["expenses:submit"];

  it("puts a freshly submitted claim on the manager", () => {
    expect(isMyExpenseStep("submitted", manager)).toBe(true);
    expect(isMyExpenseStep("submitted", finance)).toBe(true);
    expect(isMyExpenseStep("submitted", nobody)).toBe(false);
  });

  it("keeps a manager out of the finance steps", () => {
    expect(isMyExpenseStep("manager_approved", manager)).toBe(false);
    expect(isMyExpenseStep("manager_approved", finance)).toBe(true);
    expect(isMyExpenseStep("finance_approved", manager)).toBe(false);
    expect(isMyExpenseStep("finance_approved", finance)).toBe(true);
  });

  it("has nothing to do once the claim is settled", () => {
    expect(isMyExpenseStep("paid", finance)).toBe(false);
    expect(isMyExpenseStep("rejected", finance)).toBe(false);
  });
});

describe("expenseActions", () => {
  it("offers approve and reject to the manager on a submitted claim", () => {
    expect(expenseActions("submitted", ["expenses:approve:subtree"])).toEqual(["approve", "reject"]);
  });

  it("offers nothing to a manager once finance holds the claim", () => {
    expect(expenseActions("manager_approved", ["expenses:approve:subtree"])).toEqual([]);
  });

  it("lets finance pay an approved claim", () => {
    expect(expenseActions("finance_approved", ["expenses:approve:finance"])).toEqual(["pay", "reject"]);
  });

  it("offers nothing on a settled claim", () => {
    expect(expenseActions("paid", ["expenses:approve:finance"])).toEqual([]);
    expect(expenseActions("rejected", ["expenses:approve:finance"])).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// Invoices, work orders, payroll and tickets
// ---------------------------------------------------------------------------

describe("invoiceActions", () => {
  const accountant = ["invoices:draft", "invoices:issue", "payments:record"];
  const financeAdmin = [...accountant, "invoices:approve"];

  it("lets a drafter submit but not approve", () => {
    expect(invoiceActions("draft", accountant)).toEqual(["submit"]);
  });

  it("lets an approver approve a pending invoice", () => {
    expect(invoiceActions("pending_approval", financeAdmin)).toEqual(["approve", "void"]);
    expect(invoiceActions("pending_approval", accountant)).toEqual([]);
  });

  it("lets an issuer issue an approved invoice", () => {
    expect(invoiceActions("approved", accountant)).toEqual(["issue"]);
  });

  it("offers payment recording on issued and partially paid invoices", () => {
    expect(invoiceActions("issued", accountant)).toContain("record_payment");
    expect(invoiceActions("partially_paid", accountant)).toContain("record_payment");
  });

  it("offers nothing on a settled invoice", () => {
    expect(invoiceActions("paid", financeAdmin)).toEqual([]);
    expect(invoiceActions("void", financeAdmin)).toEqual([]);
  });
});

describe("workOrderTransitions", () => {
  it("walks open to in progress to done", () => {
    expect(workOrderTransitions("open")).toEqual(["in_progress", "blocked"]);
    expect(workOrderTransitions("in_progress")).toEqual(["done", "blocked"]);
    expect(workOrderTransitions("blocked")).toEqual(["in_progress"]);
  });

  it("stops at done and cancelled", () => {
    expect(workOrderTransitions("done")).toEqual([]);
    expect(workOrderTransitions("cancelled")).toEqual([]);
  });
});

describe("payrollActions", () => {
  it("needs the approval permission", () => {
    expect(payrollActions("draft", ["payroll:prepare"])).toEqual([]);
    expect(payrollActions("draft", ["payroll:approve"])).toEqual(["approve"]);
    expect(payrollActions("approved", ["payroll:approve"])).toEqual(["post"]);
    expect(payrollActions("posted", ["payroll:approve"])).toEqual([]);
  });
});

describe("ticketTransitions", () => {
  it("lets a requester reopen a recently resolved ticket", () => {
    const now = new Date("2026-08-27T00:00:00Z");
    const recent = ticketTransitions("resolved", {
      isAgent: false,
      isRequester: true,
      resolvedAt: "2026-08-25T00:00:00Z",
      now,
    });
    expect(recent).toContain("open");

    const stale = ticketTransitions("resolved", {
      isAgent: false,
      isRequester: true,
      resolvedAt: "2026-08-01T00:00:00Z",
      now,
    });
    expect(stale).not.toContain("open");
  });
});
