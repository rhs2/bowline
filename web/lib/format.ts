import type { Employee, EmployeeRef } from "./types";

const moneyFormatters = new Map<string, Intl.NumberFormat>();

export function formatMoney(amount: string | number | null | undefined, currency = "USD"): string {
  if (amount === null || amount === undefined || amount === "") return "";
  const n = typeof amount === "number" ? amount : Number(amount);
  if (Number.isNaN(n)) return String(amount);
  let fmt = moneyFormatters.get(currency);
  if (!fmt) {
    try {
      fmt = new Intl.NumberFormat("en-US", { style: "currency", currency, currencyDisplay: "code" });
    } catch {
      fmt = new Intl.NumberFormat("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
    }
    moneyFormatters.set(currency, fmt);
  }
  return fmt.format(n);
}

export function formatNumber(value: string | number | null | undefined, digits = 0): string {
  if (value === null || value === undefined || value === "") return "";
  const n = typeof value === "number" ? value : Number(value);
  if (Number.isNaN(n)) return String(value);
  return new Intl.NumberFormat("en-US", {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(n);
}

export function formatPercent(value: string | number | null | undefined): string {
  if (value === null || value === undefined || value === "") return "";
  const n = typeof value === "number" ? value : Number(value);
  if (Number.isNaN(n)) return "";
  return `${Math.round(n * 100)}%`;
}

const dateFmt = new Intl.DateTimeFormat("en-GB", {
  day: "2-digit",
  month: "short",
  year: "numeric",
  timeZone: "UTC",
});

const dateTimeFmt = new Intl.DateTimeFormat("en-GB", {
  day: "2-digit",
  month: "short",
  year: "numeric",
  hour: "2-digit",
  minute: "2-digit",
  hour12: false,
});

const timeFmt = new Intl.DateTimeFormat("en-GB", { hour: "2-digit", minute: "2-digit", hour12: false });

/** Calendar dates (YYYY-MM-DD) are rendered in UTC so they never shift by a day. */
export function formatDate(value: string | null | undefined): string {
  if (!value) return "";
  const d = value.length === 10 ? new Date(`${value}T00:00:00Z`) : new Date(value);
  if (Number.isNaN(d.getTime())) return value;
  return dateFmt.format(d);
}

export function formatDateTime(value: string | null | undefined): string {
  if (!value) return "";
  const d = new Date(value);
  if (Number.isNaN(d.getTime())) return value;
  return dateTimeFmt.format(d);
}

export function formatTime(value: string | null | undefined): string {
  if (!value) return "";
  const d = new Date(value);
  if (Number.isNaN(d.getTime())) return value;
  return timeFmt.format(d);
}

/** "in 3h", "2d ago", "just now". */
export function formatRelative(value: string | null | undefined, now: Date = new Date()): string {
  if (!value) return "";
  const d = new Date(value);
  if (Number.isNaN(d.getTime())) return value;
  const diff = d.getTime() - now.getTime();
  const abs = Math.abs(diff);
  const minutes = Math.round(abs / 60000);
  const hours = Math.round(abs / 3600000);
  const days = Math.round(abs / 86400000);
  let span: string;
  if (minutes < 1) return "just now";
  if (minutes < 60) span = `${minutes}m`;
  else if (hours < 48) span = `${hours}h`;
  else span = `${days}d`;
  return diff > 0 ? `in ${span}` : `${span} ago`;
}

/** Time left until a deadline, or how long ago it passed. */
export function formatCountdown(deadline: string, now: Date = new Date()): { text: string; overdue: boolean } {
  const d = new Date(deadline);
  const diff = d.getTime() - now.getTime();
  const overdue = diff < 0;
  const abs = Math.abs(diff);
  const h = Math.floor(abs / 3600000);
  const m = Math.floor((abs % 3600000) / 60000);
  const text = h >= 48 ? `${Math.floor(h / 24)}d ${h % 24}h` : `${h}h ${m.toString().padStart(2, "0")}m`;
  return { text: overdue ? `${text} over` : text, overdue };
}

/** Today's date as YYYY-MM-DD in local time. */
export function todayIso(now: Date = new Date()): string {
  const y = now.getFullYear();
  const m = String(now.getMonth() + 1).padStart(2, "0");
  const d = String(now.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

export function addDays(iso: string, days: number): string {
  const d = new Date(`${iso}T00:00:00Z`);
  d.setUTCDate(d.getUTCDate() + days);
  return d.toISOString().slice(0, 10);
}

/** Inclusive calendar-day span between two YYYY-MM-DD dates. */
export function daysBetween(start: string, end: string): number {
  const a = new Date(`${start}T00:00:00Z`).getTime();
  const b = new Date(`${end}T00:00:00Z`).getTime();
  if (Number.isNaN(a) || Number.isNaN(b) || b < a) return 0;
  return Math.round((b - a) / 86400000) + 1;
}

/** Convert a datetime-local input value to RFC 3339 UTC. */
export function localInputToIso(value: string): string {
  if (!value) return "";
  const d = new Date(value);
  return Number.isNaN(d.getTime()) ? "" : d.toISOString();
}

/** Convert an RFC 3339 timestamp to a datetime-local input value. */
export function isoToLocalInput(value: string | null | undefined): string {
  if (!value) return "";
  const d = new Date(value);
  if (Number.isNaN(d.getTime())) return "";
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** "in_transit" -> "In transit". */
export function humanize(value: string | null | undefined): string {
  if (!value) return "";
  const s = value.replace(/[_-]+/g, " ").trim();
  return s.charAt(0).toUpperCase() + s.slice(1);
}

export function fullName(e: Pick<Employee, "first_name" | "last_name"> | EmployeeRef | null | undefined): string {
  if (!e) return "";
  if ("name" in e) return e.name;
  return `${e.first_name} ${e.last_name}`.trim();
}

export function initials(name: string): string {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((p) => p.charAt(0).toUpperCase())
    .join("");
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function formatLocation(loc: { city?: string; country?: string; port?: string } | null | undefined): string {
  if (!loc) return "";
  return [loc.port, loc.city, loc.country].filter(Boolean).join(", ");
}

export const LEVEL_NAMES: Record<number, string> = {
  1: "Chief",
  2: "C-suite",
  3: "Director",
  4: "Manager",
  5: "Supervisor",
  6: "Specialist",
  7: "Ground",
};

export function levelName(level: number | null | undefined): string {
  if (!level) return "";
  return LEVEL_NAMES[level] ?? `Level ${level}`;
}

export const MONTH_NAMES = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
];

export function periodLabel(year: number, month: number): string {
  return `${MONTH_NAMES[month - 1] ?? month} ${year}`;
}
