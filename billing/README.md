# Bowline billing

The Java service that turns finance rows into documents people actually send:

- **Invoice PDFs.** The API posts an invoice with its customer and lines when an
  accountant issues it; the service renders the PDF and stores it, answering with the key.
- **Personnel files.** A contract, payslip, certificate or identity record for one
  employee, laid out on the same furniture as an invoice and stored under the key the
  caller gives, which is the key the `employee_documents` row already holds.
- **Customer statements.** A statement of account for one customer over a date window,
  with opening balance, every invoice and payment, and a running balance.
- **AR aging spreadsheets.** The `ar_aging` view rendered as a real `.xlsx` workbook with
  per-bucket subtotals, for finance to work in.

Spring Boot 3 on port `BILLING_BIND_PORT` (default 8081), OpenPDF for the PDFs, Apache POI
for the workbook, plain JDBC through the read-only database role, Micrometer for metrics.
The service never writes to the database.

## Running it

```sh
cd billing
./mvnw -B verify                       # compile, run the suite, package the jar

BILLING_PDF_OUTPUT=local \
INTERNAL_SERVICE_TOKEN=dev-internal-token-change-me \
  java -jar target/bowline-billing.jar
```

`./mvnw spring-boot:run` does the same without packaging. The service refuses to start
without `INTERNAL_SERVICE_TOKEN`. The connection pool starts lazily, so it boots with the
database down and reports that on `/readyz` instead of failing.

Configuration (all from the environment, names as in the repository `.env.example`):

| Variable                   | Default                                | Meaning                                                        |
|----------------------------|----------------------------------------|----------------------------------------------------------------|
| `BILLING_BIND_PORT`        | `8081`                                 | Listening port                                                 |
| `INTERNAL_SERVICE_TOKEN`   | required                               | Shared secret expected in `X-Internal-Token`                   |
| `BILLING_PDF_OUTPUT`       | `s3`                                   | `s3` or `local`                                                |
| `BILLING_LOCAL_OUTPUT_DIR` | `./out`                                | Root directory when `BILLING_PDF_OUTPUT=local`                 |
| `BILLING_DATABASE_URL`     | `jdbc:postgresql://localhost:5432/bowline` | JDBC URL for statements and AR aging                       |
| `BILLING_DATABASE_USER`    | `bowline_ro`                           | Read-only role                                                 |
| `BILLING_DATABASE_PASSWORD`| empty                                  | Password for that role                                         |
| `BILLING_COMPANY_NAME`     | `Bowline Logistics`                    | Issuer shown in document headers                               |
| `BILLING_COMPANY_ADDRESS`  | `1 Harbour Way, Port City`             | Issuer address in headers and page footers                     |
| `S3_BUCKET_PDFS`           | `bowline-pdfs`                         | Bucket for invoices and statements                             |
| `S3_BUCKET_DOCUMENTS`      | `bowline-documents`                    | Bucket for personnel files, the one the API presigns them from |
| `S3_REGION`                | `us-east-1`                            | Bucket region                                                  |
| `S3_ENDPOINT`              | empty                                  | Set for MinIO or any S3 compatible store; blank means real AWS |
| `S3_ACCESS_KEY_ID`         | empty                                  | Blank falls back to the default credential chain               |
| `S3_SECRET_ACCESS_KEY`     | empty                                  | As above                                                       |
| `S3_FORCE_PATH_STYLE`      | `0`                                    | `1` for MinIO                                                  |

## Endpoints

Every route except `/healthz`, `/readyz` and `/metrics` requires `X-Internal-Token` equal
to `INTERNAL_SERVICE_TOKEN`; anything else is a `401` problem document, refused before the
request body is read. Errors follow RFC 7807 (`application/problem+json`) with a stable
`code`: `unauthorized`, `validation_failed` (with `errors: [{field, message}]`),
`malformed_request`, `unsupported_media_type`, `method_not_allowed`, `not_found`,
`storage_unavailable`, `database_unavailable`, `internal`. Every response carries an
`X-Request-Id`, echoed from the request when the caller sends one.

### POST /render/invoice

The call the API makes when an invoice is issued. Money is a decimal string or a number,
keys are snake_case, `tax`, `amount_paid` and `tax_rate` default to zero, and `shipment`
is optional.

```sh
curl -s -X POST http://localhost:8081/render/invoice \
  -H 'X-Internal-Token: dev-internal-token-change-me' \
  -H 'Content-Type: application/json' \
  -d '{
    "invoice": {"invoice_no":"INV-2026-000412","issue_date":"2026-08-01","due_date":"2026-08-31",
                "currency":"USD","subtotal":"18420.00","tax":"1289.40","total":"19709.40",
                "amount_paid":"5000.00","notes":"Bank: Harbour Bank, account 00-1234-5678."},
    "customer": {"name":"Blue Harbour Foods Ltd.","code":"BHF",
                 "contact":{"name":"Marta Oyelaran","email":"ap@blueharbour.example","phone":"+1 555 0142"},
                 "billing_address":{"line1":"Unit 12, 400 Wharf Road","line2":"Dockside Business Park",
                                    "city":"Port City","region":"PC","postal_code":"40010","country":"Freelandia"}},
    "shipment": {"reference":"BWL-2026-000456","mode":"sea","origin":"Shanghai","destination":"Port City"},
    "lines": [
      {"seq":1,"description":"Sea freight, Shanghai to Port City, 2 x 40ft HC","quantity":"2","unit_price":"6200.00","tax_rate":"0","amount":"12400.00"},
      {"seq":2,"description":"Customs brokerage and documentation","quantity":"1","unit_price":"350.00","tax_rate":"0.07","amount":"350.00"}
    ]
  }'
```

```json
{"s3_key":"invoices/INV-2026-000412.pdf","bytes":11383}
```

Add `?inline=1` to get the bytes back as `application/pdf` instead, storing nothing. That
is what a preview uses:

```sh
curl -s -o invoice.pdf -X POST 'http://localhost:8081/render/invoice?inline=1' \
  -H 'X-Internal-Token: dev-internal-token-change-me' \
  -H 'Content-Type: application/json' --data-binary @invoice.json
file invoice.pdf     # PDF document, version 1.5, 1 pages
```

Bean validation covers each field (invoice number pattern, three-letter currency, at most
500 lines, no negative money, quantity above zero, tax rate in 0 to 1). Cross-field rules
run afterwards in `InvoiceRules` and are hard `422` failures, because a document whose
numbers do not add up must never reach a customer: the due date cannot precede the issue
date, `total` must equal `subtotal + tax`, and `amount_paid` must not exceed `total`.
Duplicate line `seq` values are rejected too. Line amounts that do not sum to the subtotal
only log a warning, since the header totals are what the ledger posted.

### POST /render/document

One personnel file. `kind` is `contract`, `payslip`, `certificate` or `id`, and only the
detail block matching it is read; the others may be left out. `s3_key` is the object key
the document must be stored under, because the `employee_documents` row already names it:
it has to sit under `employees/<employee id>/` and end in `.pdf`, which keeps a render
from writing over an invoice or escaping the bucket layout.

```sh
curl -s -X POST http://localhost:8081/render/document \
  -H 'X-Internal-Token: dev-internal-token-change-me' \
  -H 'Content-Type: application/json' \
  -d '{
    "kind": "payslip",
    "s3_key": "employees/1c9e6f2a-8d43-4f7b-b0c5-2a9d8e7f6b5c/payslip-2026-07.pdf",
    "title": "Payslip 2026-07",
    "employee": {"employee_no":"BWL-000482","name":"Priya Raman","email":"priya.raman@bowline.example",
                 "position_title":"Warehouse Supervisor","department":"Warehouse Operations",
                 "site":"Port City Terminal","pay_grade":"G5","manager_name":"Marcus Elliot",
                 "hire_date":"2022-03-14","employment_type":"full_time"},
    "payslip": {"period":"2026-07","period_start":"2026-07-01","period_end":"2026-07-31",
                "pay_date":"2026-07-31","gross":"5700.00","deductions":"1596.00","net":"4104.00",
                "currency":"USD","pay_method":"bank_transfer"}
  }'
```

```json
{"s3_key":"employees/1c9e6f2a-8d43-4f7b-b0c5-2a9d8e7f6b5c/payslip-2026-07.pdf","bytes":10909}
```

`?inline=1` returns the bytes as `application/pdf` and stores nothing, the same as it does
for an invoice. Each kind needs its own block: `contract` (job title, department, start
date, salary, employment type, and optionally pay grade, site, weekly hours and notice
days), `payslip` (period, gross, deductions, net and the period dates), `certificate`
(name and issue date, optionally issuer, expiry and reference) or `identity` for kind
`id` (document type, optionally number, authority and dates). Cross-field rules are hard
`422` failures: the block has to match the kind, a payslip's `net` must equal
`gross - deductions` and its deductions may not exceed gross, and an expiry may not fall
before the matching issue date. Fields that are not supplied are left off the page rather
than printed empty, so a record that holds only a title and a date still renders as a
clean document.

Every page says in its footer that it is generated demonstration data for a fictional
company, because these are documents that otherwise look exactly like the real thing.

### GET /statements/{customerId}.pdf?from=&to=

A statement of account, read live from the database through the read-only role: the
customer, the opening balance carried into the window, then every issued invoice and every
payment inside it in date order with a running balance.

```sh
curl -s -o statement.pdf \
  'http://localhost:8081/statements/7d5a2f0e-3b1c-4c8e-9a6d-1f2e3d4c5b6a.pdf?from=2026-07-01&to=2026-08-31' \
  -H 'X-Internal-Token: dev-internal-token-change-me'
```

The response is `application/pdf` with
`Content-Disposition: inline; filename="statement-ACME-2026-07-01-2026-08-31.pdf"`.
An unknown customer is a `404` (`not_found`); a window whose `to` precedes its `from` is a
`422` (`validation_failed`) naming the `from` field.

### GET /reports/ar-aging.xlsx?as_of=

Outstanding invoices aged into `current`, `1-30`, `31-60`, `61-90` and `90+` on the report
date, which defaults to today in UTC. The bucket rule is the one in the `ar_aging` view,
recomputed for an arbitrary `as_of`.

```sh
curl -s -o ar-aging.xlsx \
  'http://localhost:8081/reports/ar-aging.xlsx?as_of=2026-08-27' \
  -H 'X-Internal-Token: dev-internal-token-change-me'
```

The response is an OOXML workbook with
`Content-Disposition: attachment; filename="ar-aging-2026-08-27.xlsx"`. The API proxies
this route for `?format=xlsx` on its own AR aging endpoint.

### GET /healthz, /readyz and /metrics

`/healthz` returns `{"status":"ok"}` as soon as the process answers. `/readyz` borrows a
connection from the read-only pool with a one second budget and returns
`{"status":"ok","database":"ok"}` or a `503` with `{"status":"unavailable","database":"..."}`.
`/metrics` is the Prometheus scrape (Micrometer plus the JVM and Tomcat meters, tagged
`application=bowline-billing`). None of the three needs a token.

## How the documents are produced

**PDFs (OpenPDF).** `document/PdfStyles.java` holds the fonts, colours and cell factories
and `document/DocumentLayout.java` the page furniture (the A4 box, the letterhead, the
rule under it, the labelled paragraph), so an invoice, a statement and a contract look
like they came from the same company and the house style changes in one place.
`InvoicePdfRenderer` lays out the issuer block, the bill-to and shipment block, the line
table with per-line tax, the totals stack (subtotal, tax, total, paid, balance due), the
payment terms line and the notes; `StatementPdfRenderer` lays out the header, the customer
and period block, the opening/charges/payments/closing summary and the movements table;
`EmployeeDocumentPdfRenderer` lays out an employee block and then the body the kind calls
for, a terms table with numbered clauses and signature lines for a contract, a
gross/deductions/net strip over an earnings table for a payslip, a centred attestation for
a certificate and a record card for an identity document.
`PageFooter` writes "Page n of m" and the issuer line on every page in a second pass, so
multi-page invoices and contracts number correctly. `Money` formats every amount through
`BigDecimal` with the document's currency, never a `double`.

The movements table on a statement carries the reference in bold above its description in
one Details column rather than splitting them into narrow columns of their own. A
description such as "Payment by bank transfer against INV-2026-000101" needs about 220
points at the body size, which is more than a seventh of an A4 portrait text block, so
separate columns forced wraps in the middle of words.

**Spreadsheets (Apache POI).** `ArAgingWorkbook` writes an `XSSFWorkbook`: a header row, a
row per outstanding invoice, a "Totals by bucket" block with one row per bucket in aging
order, the grand total, and the report date. Subtotals are written as values rather than
formulas so any reader shows them without recalculating, and column widths are fixed
because auto-sizing needs AWT fonts that a headless container may not have.

**Storage.** `PdfStore` has two implementations chosen by `BILLING_PDF_OUTPUT`, and two
instances: invoices and statements go to `S3_BUCKET_PDFS`, personnel files to
`S3_BUCKET_DOCUMENTS`, which is the bucket the API presigns employee document downloads
from. In local mode both write into one directory and the key prefixes keep them apart.
`S3PdfStore` puts the object into its bucket and returns the key; the AWS client is
built lazily, so local mode never constructs one. `LocalPdfStore` writes below
`BILLING_LOCAL_OUTPUT_DIR`, confines the key to that root, and writes atomically through a
temporary file and a move. Either way the response is `{"s3_key": ..., "bytes": ...}`, and
a storage failure is a `502` with code `storage_unavailable`.

## Tests and checks

```sh
cd billing
./mvnw -B verify
```

61 tests, and none of them needs a database, S3 or the network. The MockMvc suites boot
the whole service once with `BILLING_PDF_OUTPUT=local` pointed at a temp directory, fake
repositories in place of the JDBC ones, a fixed clock at 2026-08-27, and a datasource URL
nothing listens on, so `/readyz` genuinely has an unreachable database to report. Rendered
PDFs are read back with PDFBox and asserted on their text and page count, and the
spreadsheet is read back with POI and asserted cell by cell, so a broken layout fails the
build rather than shipping a blank page.

Coverage: money formatting and rounding, the readiness check, the token filter and the
open probe routes, request id propagation, bean validation and the cross-field invoice
rules, invoice rendering including multi-page pagination, all four personnel layouts with
their figures read back out of the PDF and their object keys checked against the employee
prefix, statement running balances and window filtering, AR aging bucketing and workbook
totals, and the local store including the key-escape guard.

## Docker

```sh
docker build -t bowline/billing:dev billing
docker run --rm -p 8081:8081 \
  -e INTERNAL_SERVICE_TOKEN=dev-internal-token-change-me \
  -e BILLING_PDF_OUTPUT=local \
  bowline/billing:dev
```

Two stages: `eclipse-temurin:17-jdk` resolves dependencies from the pom alone before the
sources are copied, so a code change does not re-download the world, then packages the
jar. The runtime stage is `eclipse-temurin:17-jre`, runs as the non-root `billing` user,
exposes 8081, sets `-XX:MaxRAMPercentage=75.0` and headless AWT, and has a `HEALTHCHECK`
against `/healthz`.
