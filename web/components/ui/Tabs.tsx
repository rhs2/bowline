"use client";

import clsx from "clsx";
import type { ReactNode } from "react";

export interface TabItem<K extends string> {
  key: K;
  label: ReactNode;
  count?: number;
}

export function Tabs<K extends string>({
  tabs,
  value,
  onChange,
}: {
  tabs: TabItem<K>[];
  value: K;
  onChange: (key: K) => void;
}) {
  return (
    <div className="border-b border-slate-200">
      <nav className="-mb-px flex gap-4 overflow-x-auto" aria-label="Tabs" role="tablist">
        {tabs.map((t) => {
          const active = t.key === value;
          return (
            <button
              key={t.key}
              type="button"
              role="tab"
              aria-selected={active}
              onClick={() => onChange(t.key)}
              className={clsx(
                "flex shrink-0 items-center gap-2 border-b-2 px-1 py-3 text-sm font-medium",
                active
                  ? "border-accent-600 text-accent-700"
                  : "border-transparent text-slate-500 hover:border-slate-300 hover:text-slate-700",
              )}
            >
              {t.label}
              {t.count !== undefined ? (
                <span
                  className={clsx(
                    "rounded-full px-2 py-0.5 text-xs",
                    active ? "bg-accent-100 text-accent-800" : "bg-slate-100 text-slate-600",
                  )}
                >
                  {t.count}
                </span>
              ) : null}
            </button>
          );
        })}
      </nav>
    </div>
  );
}
