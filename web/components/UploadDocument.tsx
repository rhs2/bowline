"use client";

import { useState, type FormEvent } from "react";
import { api } from "@/lib/api";
import { useAction } from "@/lib/forms";
import type { PresignResponse } from "@/lib/types";
import { Button } from "./ui/Button";
import { FormError, Input, Select, type SelectOption } from "./ui/Field";
import { formatBytes } from "@/lib/format";

export interface UploadSpec {
  /** POST target returning `{upload_url, s3_key}`. */
  presignPath: string;
  /** POST target that records the document after the S3 PUT succeeded. */
  confirmPath: string;
  /** Extra fields sent with both calls (for example `employee_id`). */
  extra?: Record<string, unknown>;
  kinds: SelectOption[];
}

const MAX_BYTES = 25 * 1024 * 1024;

/**
 * Presigned upload: ask the API for a PUT URL, send the bytes straight to object
 * storage, then confirm so the API records key, size and MIME type. The file never
 * touches the API process.
 */
export function UploadDocument({ spec, onDone }: { spec: UploadSpec; onDone: () => void }) {
  const [kind, setKind] = useState(spec.kinds[0]?.value ?? "other");
  const [title, setTitle] = useState("");
  const [file, setFile] = useState<File | null>(null);
  const [stage, setStage] = useState<"idle" | "presign" | "upload" | "confirm">("idle");
  const [localError, setLocalError] = useState<string | null>(null);

  const action = useAction(
    async () => {
      if (!file) throw new Error("Choose a file");
      const mime = file.type || "application/octet-stream";
      const meta = { ...(spec.extra ?? {}), kind, title: title || file.name, mime_type: mime, size_bytes: file.size };
      setStage("presign");
      const presign = await api.post<PresignResponse>(spec.presignPath, meta);
      setStage("upload");
      const put = await fetch(presign.upload_url, { method: "PUT", body: file, headers: { "content-type": mime } });
      if (!put.ok) throw new Error(`Upload to storage failed (HTTP ${put.status})`);
      setStage("confirm");
      await api.post(spec.confirmPath, { ...meta, s3_key: presign.s3_key });
    },
    {
      successMessage: "Document uploaded",
      onSuccess: () => {
        setFile(null);
        setTitle("");
        setStage("idle");
        onDone();
      },
    },
  );

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    setLocalError(null);
    if (!file) {
      setLocalError("Choose a file to upload");
      return;
    }
    if (file.size > MAX_BYTES) {
      setLocalError(`Files must be smaller than ${formatBytes(MAX_BYTES)}`);
      return;
    }
    void action.run().finally(() => setStage("idle"));
  }

  const stageLabel =
    stage === "presign" ? "Preparing" : stage === "upload" ? "Uploading" : stage === "confirm" ? "Recording" : "Upload";

  return (
    <form onSubmit={onSubmit} className="space-y-3">
      <FormError message={localError ?? (action.error && action.error.status !== 422 ? action.error.message : null)} />
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <Select
          label="Kind"
          options={spec.kinds}
          value={kind}
          onChange={(e) => setKind(e.target.value)}
          error={action.fieldErrors.kind}
        />
        <Input
          label="Title"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          placeholder={file?.name ?? "Defaults to the file name"}
          error={action.fieldErrors.title}
        />
      </div>
      <Input
        label="File"
        type="file"
        onChange={(e) => setFile(e.target.files?.[0] ?? null)}
        hint={file ? `${file.name}, ${formatBytes(file.size)}` : undefined}
        error={action.fieldErrors.size_bytes ?? action.fieldErrors.mime_type}
      />
      <div className="flex justify-end">
        <Button type="submit" loading={action.pending}>
          {stageLabel}
        </Button>
      </div>
    </form>
  );
}
