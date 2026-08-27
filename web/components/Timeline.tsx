import type { ReactNode } from "react";
import clsx from "clsx";
import type { BadgeTone } from "./ui/Badge";

export interface TimelineItem {
  key: string;
  title: ReactNode;
  time: ReactNode;
  body?: ReactNode;
  tone?: BadgeTone;
}

const DOT: Record<BadgeTone, string> = {
  neutral: "bg-slate-400",
  info: "bg-sky-500",
  success: "bg-emerald-500",
  warning: "bg-amber-500",
  danger: "bg-red-500",
  accent: "bg-accent-600",
};

export function Timeline({ items }: { items: TimelineItem[] }) {
  return (
    <ol className="relative space-y-5 border-l border-slate-200 pl-5">
      {items.map((item) => (
        <li key={item.key} className="relative">
          <span
            className={clsx("absolute -left-[1.55rem] top-1.5 h-2.5 w-2.5 rounded-full ring-4 ring-white", DOT[item.tone ?? "neutral"])}
            aria-hidden="true"
          />
          <div className="flex flex-wrap items-baseline justify-between gap-x-3">
            <p className="text-sm font-medium text-slate-900">{item.title}</p>
            <p className="text-xs text-slate-500">{item.time}</p>
          </div>
          {item.body ? <div className="mt-1 text-sm text-slate-600">{item.body}</div> : null}
        </li>
      ))}
    </ol>
  );
}
