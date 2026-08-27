"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import clsx from "clsx";
import { visibleNav } from "@/lib/nav";

function BrandMark() {
  return (
    <Link href="/dashboard" className="flex items-center gap-2 px-4 py-5">
      <span className="flex h-8 w-8 items-center justify-center rounded-md bg-accent-600 text-sm font-bold text-white">
        B
      </span>
      <span className="text-base font-semibold tracking-tight text-slate-900">Bowline</span>
    </Link>
  );
}

function NavLinks({ permissions }: { permissions: readonly string[] }) {
  const pathname = usePathname();
  const sections = visibleNav(permissions);
  return (
    <nav className="flex-1 space-y-6 overflow-y-auto px-3 pb-6" aria-label="Main">
      {sections.map((section) => (
        <div key={section.label}>
          <p className="px-2 pb-1 text-xs font-semibold uppercase tracking-wide text-slate-400">{section.label}</p>
          <ul className="space-y-0.5">
            {section.items.map((item) => {
              const active = pathname === item.href || pathname.startsWith(`${item.href}/`);
              return (
                <li key={item.href}>
                  <Link
                    href={item.href}
                    aria-current={active ? "page" : undefined}
                    className={clsx(
                      "block rounded-md px-2 py-2 text-sm font-medium transition",
                      active ? "bg-accent-50 text-accent-800" : "text-slate-700 hover:bg-slate-100",
                    )}
                  >
                    {item.label}
                  </Link>
                </li>
              );
            })}
          </ul>
        </div>
      ))}
    </nav>
  );
}

export function Sidebar({
  open,
  onClose,
  permissions,
}: {
  open: boolean;
  onClose: () => void;
  permissions: readonly string[];
}) {
  return (
    <>
      <aside className="fixed inset-y-0 left-0 z-30 hidden w-64 flex-col border-r border-slate-200 bg-white lg:flex">
        <BrandMark />
        <NavLinks permissions={permissions} />
      </aside>

      {open ? (
        <div className="fixed inset-0 z-40 lg:hidden" role="dialog" aria-modal="true" aria-label="Navigation">
          <div className="absolute inset-0 bg-slate-900/50" onClick={onClose} aria-hidden="true" />
          <aside className="absolute inset-y-0 left-0 flex w-72 max-w-[85vw] flex-col bg-white shadow-xl">
            <div className="flex items-center justify-between pr-3">
              <BrandMark />
              <button
                type="button"
                onClick={onClose}
                className="rounded p-2 text-slate-500 hover:bg-slate-100"
                aria-label="Close navigation"
              >
                <svg viewBox="0 0 20 20" className="h-5 w-5" fill="currentColor" aria-hidden="true">
                  <path d="M6.28 5.22a.75.75 0 00-1.06 1.06L8.94 10l-3.72 3.72a.75.75 0 101.06 1.06L10 11.06l3.72 3.72a.75.75 0 101.06-1.06L11.06 10l3.72-3.72a.75.75 0 00-1.06-1.06L10 8.94 6.28 5.22z" />
                </svg>
              </button>
            </div>
            <NavLinks permissions={permissions} />
          </aside>
        </div>
      ) : null}
    </>
  );
}
