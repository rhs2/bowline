import { describe, expect, it } from "vitest";
import {
  addAmounts,
  compareAmounts,
  fromCents,
  isAmount,
  ledgerTotals,
  subtractAmounts,
  toCents,
  validateEntryLines,
} from "./ledger";

describe("toCents and fromCents", () => {
  it("parses decimal strings exactly", () => {
    expect(toCents("0")).toBe(0);
    expect(toCents("1250.00")).toBe(125000);
    expect(toCents("0.1")).toBe(10);
    expect(toCents("0.05")).toBe(5);
    expect(toCents("-12.34")).toBe(-1234);
    expect(toCents("")).toBe(0);
  });

  it("rejects anything that is not a two-decimal amount", () => {
    expect(() => toCents("1.234")).toThrow();
    expect(() => toCents("abc")).toThrow();
    expect(() => toCents("1,000.00")).toThrow();
  });

  it("round-trips through cents", () => {
    for (const value of ["0.00", "0.07", "9.90", "1250.00", "-3.05"]) {
      expect(fromCents(toCents(value))).toBe(Number(value).toFixed(2));
    }
  });
});

describe("isAmount", () => {
  it("accepts blank and non-negative two-decimal amounts", () => {
    expect(isAmount("")).toBe(true);
    expect(isAmount("10")).toBe(true);
    expect(isAmount("10.5")).toBe(true);
    expect(isAmount("10.55")).toBe(true);
  });

  it("rejects negatives and over-precise values", () => {
    expect(isAmount("-1.00")).toBe(false);
    expect(isAmount("1.005")).toBe(false);
    expect(isAmount("one")).toBe(false);
  });
});

describe("addAmounts and subtractAmounts", () => {
  it("adds without binary float drift", () => {
    expect(addAmounts("0.10", "0.20")).toBe("0.30");
    expect(addAmounts("0.1", "0.2")).toBe("0.30");
    expect(addAmounts("1250.00", "99.99", "0.01")).toBe("1350.00");
  });

  it("subtracts and can go negative", () => {
    expect(subtractAmounts("100.00", "40.50")).toBe("59.50");
    expect(subtractAmounts("10.00", "12.50")).toBe("-2.50");
  });

  it("compares by value, not by string", () => {
    expect(compareAmounts("10.00", "9.99")).toBe(1);
    expect(compareAmounts("9.99", "10.00")).toBe(-1);
    expect(compareAmounts("10", "10.00")).toBe(0);
  });
});

describe("ledgerTotals", () => {
  it("sums both sides and reports the difference", () => {
    const totals = ledgerTotals([
      { debit: "100.00", credit: "0" },
      { debit: "0", credit: "60.00" },
    ]);
    expect(totals.debit).toBe("100.00");
    expect(totals.credit).toBe("60.00");
    expect(totals.difference).toBe("40.00");
    expect(totals.balanced).toBe(false);
  });

  it("is balanced only when both sides match and are non-zero", () => {
    expect(ledgerTotals([{ debit: "50.00", credit: "0" }, { debit: "0", credit: "50.00" }]).balanced).toBe(true);
    expect(ledgerTotals([{ debit: "0", credit: "0" }]).balanced).toBe(false);
    expect(ledgerTotals([]).balanced).toBe(false);
  });

  it("treats malformed input as zero rather than throwing", () => {
    const totals = ledgerTotals([{ debit: "oops", credit: "" }, { debit: "5.00", credit: "" }]);
    expect(totals.debit).toBe("5.00");
    expect(totals.credit).toBe("0.00");
  });
});

describe("validateEntryLines", () => {
  const good = [
    { account_code: "1000", debit: "500.00", credit: "0" },
    { account_code: "4000", debit: "0", credit: "500.00" },
  ];

  it("accepts a balanced two-sided entry", () => {
    const result = validateEntryLines(good);
    expect(result.ok).toBe(true);
    expect(result.problems).toEqual([]);
    expect(result.totals.balanced).toBe(true);
  });

  it("refuses an entry with fewer than two lines", () => {
    const result = validateEntryLines([{ account_code: "1000", debit: "10.00", credit: "0" }]);
    expect(result.ok).toBe(false);
    expect(result.problems.some((p) => p.message.includes("at least two lines"))).toBe(true);
  });

  it("refuses a line that is both a debit and a credit", () => {
    const result = validateEntryLines([
      { account_code: "1000", debit: "10.00", credit: "10.00" },
      { account_code: "4000", debit: "0", credit: "0" },
    ]);
    expect(result.ok).toBe(false);
    expect(result.problems.some((p) => p.index === 0)).toBe(true);
  });

  it("reports the amount an entry is out of balance by", () => {
    const result = validateEntryLines([
      { account_code: "1000", debit: "500.00", credit: "0" },
      { account_code: "4000", debit: "0", credit: "450.00" },
    ]);
    expect(result.ok).toBe(false);
    expect(result.problems.some((p) => p.message.includes("50.00"))).toBe(true);
    expect(result.totals.difference).toBe("50.00");
  });

  it("requires an account on every line", () => {
    const result = validateEntryLines([
      { account_code: "", debit: "500.00", credit: "0" },
      { account_code: "4000", debit: "0", credit: "500.00" },
    ]);
    expect(result.ok).toBe(false);
    expect(result.problems.some((p) => p.index === 0 && p.message.includes("account"))).toBe(true);
  });

  it("refuses amounts with more than two decimals", () => {
    const result = validateEntryLines([
      { account_code: "1000", debit: "500.123", credit: "0" },
      { account_code: "4000", debit: "0", credit: "500.00" },
    ]);
    expect(result.ok).toBe(false);
    expect(result.problems.some((p) => p.index === 0 && p.message.includes("two decimals"))).toBe(true);
  });
});
