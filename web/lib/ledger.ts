/**
 * Ledger arithmetic in integer cents. Amounts travel as decimal strings; nothing here
 * ever goes through a binary float, so "0.10" + "0.20" is exactly "0.30".
 */

export interface LedgerLineLike {
  debit: string | number;
  credit: string | number;
}

export interface LedgerTotals {
  debit: string;
  credit: string;
  difference: string;
  balanced: boolean;
}

const DECIMAL = /^-?\d+(\.\d{0,2})?$/;

/** Parse a decimal string (up to 2 places) into integer cents. Throws on garbage. */
export function toCents(value: string | number): number {
  const s = String(value ?? "").trim();
  if (s === "") return 0;
  if (!DECIMAL.test(s)) throw new Error(`invalid amount: ${s}`);
  const negative = s.startsWith("-");
  const [whole, frac = ""] = (negative ? s.slice(1) : s).split(".");
  const cents = Number(whole) * 100 + Number((frac + "00").slice(0, 2));
  return negative ? -cents : cents;
}

/** Whether a string is a well-formed non-negative amount (blank counts as zero). */
export function isAmount(value: string | number): boolean {
  const s = String(value ?? "").trim();
  return s === "" || (DECIMAL.test(s) && !s.startsWith("-"));
}

/** Format integer cents as a decimal string with two places. */
export function fromCents(cents: number): string {
  const sign = cents < 0 ? "-" : "";
  const abs = Math.abs(Math.round(cents));
  const whole = Math.floor(abs / 100);
  const frac = abs % 100;
  return `${sign}${whole}.${frac.toString().padStart(2, "0")}`;
}

export function addAmounts(...values: Array<string | number>): string {
  return fromCents(values.reduce<number>((acc, v) => acc + toCents(v), 0));
}

export function subtractAmounts(a: string | number, b: string | number): string {
  return fromCents(toCents(a) - toCents(b));
}

export function compareAmounts(a: string | number, b: string | number): number {
  return Math.sign(toCents(a) - toCents(b));
}

/** Sum debits and credits; `balanced` is true only when both sides are equal and non-zero. */
export function ledgerTotals(lines: readonly LedgerLineLike[]): LedgerTotals {
  let debit = 0;
  let credit = 0;
  for (const line of lines) {
    debit += safeCents(line.debit);
    credit += safeCents(line.credit);
  }
  return {
    debit: fromCents(debit),
    credit: fromCents(credit),
    difference: fromCents(debit - credit),
    balanced: debit === credit && debit > 0,
  };
}

function safeCents(value: string | number): number {
  try {
    return toCents(value);
  } catch {
    return 0;
  }
}

export interface LedgerLineProblem {
  index: number;
  message: string;
}

/**
 * Client-side validation mirroring the database constraints on journal_lines:
 * at least two lines, every line has exactly one non-zero side, sides are
 * well-formed amounts, and the entry balances.
 */
export function validateEntryLines(
  lines: readonly (LedgerLineLike & { account_code: string })[],
): { ok: boolean; problems: LedgerLineProblem[]; totals: LedgerTotals } {
  const problems: LedgerLineProblem[] = [];
  lines.forEach((line, index) => {
    if (!line.account_code) problems.push({ index, message: "Choose an account" });
    if (!isAmount(line.debit) || !isAmount(line.credit)) {
      problems.push({ index, message: "Amounts must be numbers with up to two decimals" });
      return;
    }
    const d = toCents(line.debit);
    const c = toCents(line.credit);
    if (d > 0 && c > 0) problems.push({ index, message: "A line is either a debit or a credit" });
    if (d === 0 && c === 0) problems.push({ index, message: "Enter a debit or a credit" });
  });
  if (lines.length < 2) problems.push({ index: -1, message: "An entry needs at least two lines" });
  const totals = ledgerTotals(lines);
  if (!totals.balanced && lines.length >= 2) {
    problems.push({ index: -1, message: `Entry is out of balance by ${totals.difference}` });
  }
  return { ok: problems.length === 0, problems, totals };
}
