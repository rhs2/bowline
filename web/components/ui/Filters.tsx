import type { ReactNode } from "react";
import clsx from "clsx";

/** Horizontal filter bar that wraps on small screens. */
export function FilterBar({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div className={clsx("mb-4 flex flex-wrap items-end gap-3", className)}>{children}</div>
  );
}

export function SearchInput({
  value,
  onChange,
  placeholder = "Search",
  className,
}: {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  className?: string;
}) {
  return (
    <input
      type="search"
      value={value}
      onChange={(e) => onChange(e.target.value)}
      placeholder={placeholder}
      aria-label={placeholder}
      className={clsx(
        "h-10 rounded-md border border-slate-300 bg-white px-3 text-sm text-slate-900 shadow-sm placeholder:text-slate-400 focus:outline-none focus:ring-2 focus:ring-accent-500",
        className ?? "w-full sm:w-64",
      )}
    />
  );
}

/** Pill-style single choice, used for status chips on boards. */
export function Chips<T extends string>({
  options,
  value,
  onChange,
  allLabel = "All",
}: {
  options: Array<{ value: T; label: string }>;
  value: T | "";
  onChange: (value: T | "") => void;
  allLabel?: string;
}) {
  const chip = (active: boolean) =>
    clsx(
      "rounded-full border px-3 py-1 text-xs font-medium transition",
      active
        ? "border-accent-600 bg-accent-600 text-white"
        : "border-slate-300 bg-white text-slate-700 hover:bg-slate-50",
    );
  return (
    <div className="flex flex-wrap gap-2">
      <button type="button" className={chip(value === "")} onClick={() => onChange("")}>
        {allLabel}
      </button>
      {options.map((o) => (
        <button key={o.value} type="button" className={chip(value === o.value)} onClick={() => onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}
