"use client";

import { useEffect, useRef, useState } from "react";
import Link from "next/link";
import { useMe } from "@/lib/me";
import { useQuery } from "@/lib/hooks";
import { fullName, humanize } from "@/lib/format";
import type { ListEnvelope, Thread } from "@/lib/types";
import { Avatar } from "@/components/ui/Avatar";

const UNREAD_POLL_MS = 60000;

export function TopBar({ onMenu }: { onMenu: () => void }) {
  const { me, employee, roles } = useMe();
  const unread = useQuery<ListEnvelope<Thread>>(me ? "comms/threads" : null, {
    query: { unread: 1, per_page: 1 },
  });
  const reload = unread.reload;
  useEffect(() => {
    if (!me) return;
    const id = setInterval(reload, UNREAD_POLL_MS);
    return () => clearInterval(id);
  }, [me, reload]);
  const unreadCount = unread.data?.total ?? 0;

  const [open, setOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const onClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onClick);
    return () => document.removeEventListener("mousedown", onClick);
  }, [open]);

  const name = employee ? fullName(employee) : (me?.user.email ?? "");

  async function signOut() {
    try {
      await fetch("/api/auth/logout", { method: "POST", credentials: "same-origin" });
    } finally {
      window.location.assign("/login");
    }
  }

  return (
    <header className="sticky top-0 z-20 flex h-14 items-center gap-3 border-b border-slate-200 bg-white px-4 sm:px-6">
      <button
        type="button"
        onClick={onMenu}
        className="rounded p-2 text-slate-600 hover:bg-slate-100 lg:hidden"
        aria-label="Open navigation"
      >
        <svg viewBox="0 0 20 20" className="h-5 w-5" fill="currentColor" aria-hidden="true">
          <path d="M3 5.75A.75.75 0 013.75 5h12.5a.75.75 0 010 1.5H3.75A.75.75 0 013 5.75zm0 4.25a.75.75 0 01.75-.75h12.5a.75.75 0 010 1.5H3.75A.75.75 0 013 10zm0 4.25a.75.75 0 01.75-.75h12.5a.75.75 0 010 1.5H3.75a.75.75 0 01-.75-.75z" />
        </svg>
      </button>
      <span className="text-sm font-semibold text-slate-900 lg:hidden">Bowline</span>
      <div className="flex-1" />

      <Link
        href="/inbox"
        className="relative rounded p-2 text-slate-600 hover:bg-slate-100"
        aria-label={unreadCount > 0 ? `Inbox, ${unreadCount} unread` : "Inbox"}
      >
        <svg viewBox="0 0 20 20" className="h-5 w-5" fill="currentColor" aria-hidden="true">
          <path d="M3 4a2 2 0 00-2 2v1.161l8.441 4.221a1.25 1.25 0 001.118 0L19 7.162V6a2 2 0 00-2-2H3z" />
          <path d="M19 8.839l-7.77 3.885a2.75 2.75 0 01-2.46 0L1 8.839V14a2 2 0 002 2h14a2 2 0 002-2V8.839z" />
        </svg>
        {unreadCount > 0 ? (
          <span className="absolute -right-0.5 -top-0.5 min-w-[1.25rem] rounded-full bg-accent-600 px-1 text-center text-[11px] font-semibold leading-5 text-white">
            {unreadCount > 99 ? "99+" : unreadCount}
          </span>
        ) : null}
      </Link>

      <div className="relative" ref={menuRef}>
        <button
          type="button"
          onClick={() => setOpen((o) => !o)}
          className="flex items-center gap-2 rounded-md px-2 py-1 hover:bg-slate-100"
          aria-haspopup="menu"
          aria-expanded={open}
        >
          <Avatar name={name || "?"} size="sm" />
          <span className="hidden text-sm font-medium text-slate-800 sm:block">{name}</span>
        </button>
        {open ? (
          <div
            role="menu"
            className="absolute right-0 mt-1 w-64 rounded-md border border-slate-200 bg-white py-1 shadow-lg"
          >
            <div className="border-b border-slate-100 px-3 py-2">
              <p className="truncate text-sm font-medium text-slate-900">{name}</p>
              {employee ? (
                <p className="truncate text-xs text-slate-500">
                  {employee.title}
                  {employee.department_name ? `, ${employee.department_name}` : ""}
                </p>
              ) : null}
              {roles.length > 0 ? (
                <p className="mt-1 truncate text-xs text-slate-500">{roles.map(humanize).join(", ")}</p>
              ) : null}
            </div>
            {employee ? (
              <Link
                href={`/people/${employee.id}`}
                role="menuitem"
                className="block px-3 py-2 text-sm text-slate-700 hover:bg-slate-50"
                onClick={() => setOpen(false)}
              >
                My profile
              </Link>
            ) : null}
            <Link
              href="/change-password"
              role="menuitem"
              className="block px-3 py-2 text-sm text-slate-700 hover:bg-slate-50"
              onClick={() => setOpen(false)}
            >
              Change password
            </Link>
            <button
              type="button"
              role="menuitem"
              onClick={() => void signOut()}
              className="block w-full px-3 py-2 text-left text-sm text-slate-700 hover:bg-slate-50"
            >
              Sign out
            </button>
          </div>
        ) : null}
      </div>
    </header>
  );
}
