"use client";

import type { ReactNode } from "react";
import clsx from "clsx";
import type { ApiError } from "@/lib/api";
import { EmptyState, ErrorState } from "./States";
import { Skeleton } from "./Skeleton";

export interface Column<T> {
  key: string;
  header: ReactNode;
  render: (row: T) => ReactNode;
  className?: string;
  align?: "left" | "right" | "center";
  /** Hidden below the `md` breakpoint to keep phones readable. */
  hideOnMobile?: boolean;
}

export interface PaginationProps {
  page: number;
  perPage: number;
  total: number;
  onPage: (page: number) => void;
}

export interface DataTableProps<T> {
  columns: Column<T>[];
  rows: T[];
  rowKey: (row: T) => string;
  loading?: boolean;
  error?: ApiError | null;
  empty?: ReactNode;
  onRowClick?: (row: T) => void;
  pagination?: PaginationProps;
  footer?: ReactNode;
  dense?: boolean;
}

export function DataTable<T>({
  columns,
  rows,
  rowKey,
  loading = false,
  error = null,
  empty,
  onRowClick,
  pagination,
  footer,
  dense = false,
}: DataTableProps<T>) {
  const cell = dense ? "px-3 py-2" : "px-4 py-3";
  const showSkeleton = loading && rows.length === 0;

  return (
    <div className="overflow-hidden rounded-lg border border-slate-200 bg-white shadow-card">
      <div className="overflow-x-auto">
        <table className="min-w-full divide-y divide-slate-200 text-sm">
          <thead className="bg-slate-50">
            <tr>
              {columns.map((c) => (
                <th
                  key={c.key}
                  scope="col"
                  className={clsx(
                    cell,
                    "text-left text-xs font-semibold uppercase tracking-wide text-slate-500",
                    c.align === "right" && "text-right",
                    c.align === "center" && "text-center",
                    c.hideOnMobile && "hidden md:table-cell",
                    c.className,
                  )}
                >
                  {c.header}
                </th>
              ))}
            </tr>
          </thead>
          <tbody className={clsx("divide-y divide-slate-100", loading && rows.length > 0 && "opacity-60")}>
            {showSkeleton
              ? Array.from({ length: 5 }).map((_, i) => (
                  <tr key={`s-${i}`}>
                    {columns.map((c) => (
                      <td key={c.key} className={clsx(cell, c.hideOnMobile && "hidden md:table-cell")}>
                        <Skeleton className="h-4 w-3/4" />
                      </td>
                    ))}
                  </tr>
                ))
              : rows.map((row) => (
                  <tr
                    key={rowKey(row)}
                    onClick={onRowClick ? () => onRowClick(row) : undefined}
                    className={clsx(onRowClick && "cursor-pointer hover:bg-slate-50")}
                  >
                    {columns.map((c) => (
                      <td
                        key={c.key}
                        className={clsx(
                          cell,
                          "align-top text-slate-800",
                          c.align === "right" && "text-right tabular-nums",
                          c.align === "center" && "text-center",
                          c.hideOnMobile && "hidden md:table-cell",
                          c.className,
                        )}
                      >
                        {c.render(row)}
                      </td>
                    ))}
                  </tr>
                ))}
          </tbody>
          {footer ? <tfoot className="bg-slate-50 font-medium">{footer}</tfoot> : null}
        </table>
      </div>
      {!loading && error ? (
        <div className="p-4">
          <ErrorState error={error} />
        </div>
      ) : null}
      {!loading && !error && rows.length === 0 ? (
        <div className="p-8">{empty ?? <EmptyState title="Nothing here yet" />}</div>
      ) : null}
      {pagination ? <Pagination {...pagination} /> : null}
    </div>
  );
}

export function Pagination({ page, perPage, total, onPage }: PaginationProps) {
  const pages = Math.max(1, Math.ceil(total / Math.max(1, perPage)));
  if (total <= perPage) return null;
  const from = (page - 1) * perPage + 1;
  const to = Math.min(total, page * perPage);
  return (
    <nav
      className="flex items-center justify-between gap-3 border-t border-slate-200 px-4 py-3 text-sm"
      aria-label="Pagination"
    >
      <p className="text-slate-600">
        {from} to {to} of {total}
      </p>
      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={() => onPage(page - 1)}
          disabled={page <= 1}
          className="rounded-md border border-slate-300 px-3 py-1.5 font-medium text-slate-700 hover:bg-slate-50 disabled:cursor-not-allowed disabled:text-slate-400"
        >
          Previous
        </button>
        <span className="text-slate-600">
          Page {page} of {pages}
        </span>
        <button
          type="button"
          onClick={() => onPage(page + 1)}
          disabled={page >= pages}
          className="rounded-md border border-slate-300 px-3 py-1.5 font-medium text-slate-700 hover:bg-slate-50 disabled:cursor-not-allowed disabled:text-slate-400"
        >
          Next
        </button>
      </div>
    </nav>
  );
}
