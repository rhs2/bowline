"use client";

import { useState } from "react";
import { useMe } from "@/lib/me";
import { useQuery } from "@/lib/hooks";
import { asItems } from "@/lib/api";
import { documentKindOptions } from "@/lib/options";
import type { EmployeeDocument, ListEnvelope } from "@/lib/types";
import { PageHeader } from "@/components/ui/PageHeader";
import { Card, CardBody, CardHeader } from "@/components/ui/Card";
import { EmptyState } from "@/components/ui/States";
import { DocumentList, type DocumentRow } from "@/components/DocumentList";
import { UploadDocument } from "@/components/UploadDocument";
import { EmployeePicker, type EmployeeOption } from "@/components/pickers/EmployeePicker";

export default function DocumentsPage() {
  const { employee, has } = useMe();
  const hrAdmin = has("documents:manage:all");
  const [subject, setSubject] = useState<EmployeeOption | null>(null);

  const targetId = subject?.id ?? employee?.id ?? null;
  const viewingSelf = targetId !== null && targetId === employee?.id;

  const docs = useQuery<ListEnvelope<EmployeeDocument> | EmployeeDocument[]>(targetId ? "hr/documents" : null, {
    query: { employee_id: targetId },
  });
  const rows: DocumentRow[] = asItems(docs.data);

  // Managers see the list for their subtree but not the files themselves, so the
  // download column is only offered for your own record or to HR admins.
  const canDownload = viewingSelf || hrAdmin;

  return (
    <div>
      <PageHeader
        title="Documents"
        description="Contracts, identity documents, certificates and payslips. Files are private to the employee and to HR."
      />

      {hrAdmin ? (
        <Card className="mb-4">
          <CardHeader
            title="Whose file"
            description="Choose an employee to see their documents, or clear the field to see your own."
          />
          <CardBody>
            <div className="max-w-md">
              <EmployeePicker label="Employee" value={subject} onChange={setSubject} />
            </div>
          </CardBody>
        </Card>
      ) : null}

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
        <div className="lg:col-span-2">
          <DocumentList
            documents={rows}
            downloadPath={canDownload ? (d) => `hr/documents/${d.id}/download` : null}
            loading={docs.loading}
            error={docs.error}
            empty={
              <EmptyState
                title="No documents"
                description={
                  viewingSelf
                    ? "Nothing has been filed for you yet. HR uploads contracts, certificates and payslips here."
                    : "Nothing has been filed for this employee yet."
                }
              />
            }
          />
        </div>

        {hrAdmin && targetId ? (
          <Card>
            <CardHeader
              title="Upload"
              description={`Files are sent straight to storage, then recorded against ${viewingSelf ? "your record" : (subject?.name ?? "the employee")}.`}
            />
            <CardBody>
              <UploadDocument
                spec={{
                  presignPath: "hr/documents/presign",
                  confirmPath: "hr/documents",
                  extra: { employee_id: targetId },
                  kinds: documentKindOptions,
                }}
                onDone={() => docs.reload()}
              />
            </CardBody>
          </Card>
        ) : (
          <Card>
            <CardHeader title="About these files" />
            <CardBody>
              <p className="text-sm text-slate-600">
                Only you and HR administrators can open your documents. Your manager can see that a document exists but
                cannot download it. If a document is missing or wrong, open a support ticket in the HR category.
              </p>
            </CardBody>
          </Card>
        )}
      </div>
    </div>
  );
}
