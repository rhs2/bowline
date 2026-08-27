"use client";

import type { ReactNode } from "react";
import type { ApiError } from "@/lib/api";
import { formatBytes, formatDateTime, humanize } from "@/lib/format";
import { DataTable, type Column } from "./ui/DataTable";
import { EmptyState } from "./ui/States";
import { DownloadButton } from "./DownloadButton";

export interface DocumentRow {
  id: string;
  kind: string;
  title: string;
  mime_type: string;
  size_bytes: number;
  created_at: string;
}

/**
 * Shared table for HR and shipment documents. `downloadPath` is null when the caller
 * may see that a document exists but not open it (managers see their subtree's list
 * without download URLs, see docs/DOMAIN.md).
 */
export function DocumentList({
  documents,
  downloadPath,
  loading,
  error,
  empty,
}: {
  documents: DocumentRow[];
  downloadPath: ((doc: DocumentRow) => string) | null;
  loading?: boolean;
  error?: ApiError | null;
  empty?: ReactNode;
}) {
  const columns: Column<DocumentRow>[] = [
    { key: "title", header: "Title", render: (d) => <span className="font-medium text-slate-900">{d.title}</span> },
    { key: "kind", header: "Kind", render: (d) => (d.kind === "id" ? "ID document" : humanize(d.kind)) },
    { key: "size", header: "Size", render: (d) => formatBytes(d.size_bytes), hideOnMobile: true, align: "right" },
    { key: "added", header: "Added", render: (d) => formatDateTime(d.created_at), hideOnMobile: true },
    {
      key: "download",
      header: "",
      align: "right",
      render: (d) =>
        downloadPath ? (
          <DownloadButton path={downloadPath(d)}>Download</DownloadButton>
        ) : (
          <span className="text-xs text-slate-400">Not available to you</span>
        ),
    },
  ];

  return (
    <DataTable
      columns={columns}
      rows={documents}
      rowKey={(d) => d.id}
      loading={loading}
      error={error}
      dense
      empty={empty ?? <EmptyState title="No documents" description="Nothing has been filed here yet." />}
    />
  );
}
