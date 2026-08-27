# Bowline architecture

Bowline is the operations and workforce platform for a mid-size freight forwarder:
about 260 people from the CEO to the drivers and dock crews, moving sea, air and
road shipments for a few hundred customer accounts. One system covers the company
hierarchy, identity and access, HR, freight operations, double-entry finance,
internal mail, the support desk and the audit trail. It is the 2026 re-engineering
of an internal platform first built in 2022 to 2023 (React, Node.js, SQL); the domain
model carried over, everything else was rebuilt around a Rust core.

The name comes from the bowline knot: a loop that holds under load and never jams.

## Design goals

1. **One source of truth for "who reports to whom".** Every permission that says
   "my team", "my department" or "everyone below me" is evaluated against the same
   materialised reporting path. There is no second copy of the org chart.
2. **Correct money.** The ledger is double entry, balanced by a database constraint,
   immutable once posted, and locked per fiscal period. Invoices, payments, expenses
   and payroll are all just sources of journal entries.
3. **Every write is attributable.** Each mutation carries the acting user, request id
   and a before/after snapshot into an append-only audit log.
4. **Boring to operate.** Twelve-factor services, health and readiness probes,
   Prometheus metrics, structured logs with request ids, one command for local dev,
   one pipeline to production.
5. **Fast where it matters.** The request path (auth, authorisation, scoped queries)
   is Rust on Tokio; the heavy or specialised jobs (PDF rendering, spreadsheets,
   forecasting) sit in services written in the language best suited to them.

## Service map

```
                       +------------------+
   browser  ---------> |  web  (Next.js)  |  role-aware UI, SSR + client fetch
                       +--------+---------+
                                | HTTPS /api/v1  (JWT bearer)
                                v
   bowctl (Go) ------> +------------------+      +--------------------+
                       |  api  (Rust)     |----->| billing  (Java)    |  invoice PDFs,
                       |  Axum + SQLx     |      | Spring Boot        |  AR aging xlsx
                       |  identity, org,  |      +--------------------+
                       |  hr, ops,        |      +--------------------+
                       |  finance, comms, |----->| analytics (Python) |  delay risk,
                       |  audit           |      | FastAPI + sklearn  |  volume forecast
                       +---+--------+-----+      +--------------------+
                           |        |
                 PostgreSQL 16      S3 (documents, PDFs)
                 (ltree, citext)
                           ^
                           |  outbox table, SKIP LOCKED
                       +---+--------------+
                       | notify (Go)      |  email delivery: SES in prod,
                       +------------------+  Mailpit locally
```

| Service     | Language / framework                | Owns                                                                 |
|-------------|-------------------------------------|----------------------------------------------------------------------|
| `api`       | Rust 1.98, Axum 0.7, SQLx 0.8, Tokio | All business rules, authz, migrations, OpenAPI, seed data             |
| `web`       | TypeScript, Next.js 15, Tailwind    | Login, dashboards, org chart, inbox, tickets, HR, ops, finance, admin |
| `billing`   | Java 17, Spring Boot 3, OpenPDF, POI | Invoice PDF rendering, customer statements, AR aging spreadsheets      |
| `analytics` | Python 3.12, FastAPI, scikit-learn   | Shipment delay-risk scoring, weekly volume forecast                   |
| `tools`     | Go 1.27                             | `notify` outbox worker (SMTP/SES), `bowctl` operator CLI              |
| `infra`     | Terraform (HCL), GitHub Actions     | AWS environment, CI, image build and deploy                          |
| `db`        | SQL                                 | Versioned migrations, reference data, integrity triggers             |

The API is the only writer of business data. `billing` and `analytics` read the
database through a read-only role and are called by the API over HTTP; `notify`
owns the `notifications` outbox table only.

## Request path

1. `web` (or `bowctl`) sends a request with a short-lived access token.
2. `api` middleware: request id, rate limit (per IP and per user), JWT validation,
   principal load (user, employee, roles, permission set, reporting path). The
   principal is cached in Redis for 60 seconds, invalidated on role change.
3. Handler calls a domain service. The service computes a **scope** for the action
   (`Self`, `Subtree`, `Department`, `All`) from the principal's permissions, and every
   query is written against that scope. There are no unscoped list endpoints.
4. Writes run in one transaction: business rows, audit row, and any outbox row.
5. Response is JSON; errors are RFC 7807 problem documents with a stable `code`.

## Identity and access

**Authentication.** Argon2id password hashes (m=64MiB, t=3, p=1). Access tokens are
HS256 JWTs valid 15 minutes and carry only the user id and a token version. Refresh
tokens are 32 random bytes, stored hashed, valid 30 days, rotated on every use, with
reuse detection (a replayed refresh token revokes the whole family). Five failed
logins lock the account for 15 minutes. Passwords must be changed on first login.

**Roles and permissions.** Roles are named bundles of permission keys
(`leave:approve:subtree`, `invoices:issue`, `messages:broadcast:company`, ...). A user
has one or more roles; the effective permission set is the union. The catalogue lives
in `db/migrations/0008_reference_data.sql` and is documented in `DOMAIN.md`.

**Hierarchy scope.** `employees.path` is a PostgreSQL `ltree` holding the chain of
ids from the CEO to that employee, maintained by triggers (re-parenting an employee
rewrites the subtree in one statement, cycles are rejected). "Everyone below me" is
`path <@ :my_path`, which is a GiST index lookup, not a recursive query. Permission
suffixes select the scope: `:self`, `:subtree`, `:department`, `:all`.

**Chain of command.** Levels 1 to 7: CEO, C-suite, director, manager, supervisor,
specialist, ground staff. Approval chains follow `manager_id`; broadcast rights follow
level and permission (an executive may address the whole company, a director or
manager only their own subtree).

## Messaging rules

A sender may message a recipient if any of these hold:

- the recipient is the support desk (creates or updates a ticket);
- the recipient is the sender's manager, or reports directly to the sender;
- both are in the same department;
- the sender holds `messages:send:subtree` and the recipient is below them;
- the sender holds `messages:broadcast:company`.

Announcements fan out to the audience at send time (company, a department, or a
subtree), so the inbox query is a plain participant lookup. Every message also writes
an `email` notification to the outbox; `notify` delivers it.

## Approval workflows

| Flow          | Steps                                                                                     |
|---------------|-------------------------------------------------------------------------------------------|
| Leave         | employee requests -> direct manager approves or rejects; HR admin may override            |
| Expense       | employee submits -> manager approves -> finance approves -> paid (journal entry posted)   |
| Invoice       | accountant drafts -> issued; totals >= 50,000 need `invoices:approve` before issue         |
| Payroll       | HR/accountant prepares run -> `payroll:approve` (CFO) -> posted to ledger                 |
| Period close  | `periods:close` locks a month; posting into a closed period is rejected by trigger        |
| Shipment      | draft -> booked -> picked_up -> in_transit -> customs -> out_for_delivery -> delivered      |
| Ticket        | open -> triaged -> in_progress -> waiting_on_requester -> resolved -> closed              |

State machines are enforced in the API (`transition()` tables), and the important
ones are double-checked by database triggers (ledger balance, closed periods,
immutable journal lines).

## Finance model

- Chart of accounts with five root types; every account carries a code and a type.
- `journal_entries` + `journal_lines`; a deferred constraint trigger asserts
  `sum(debit) = sum(credit)` per entry at commit; lines are immutable (corrections are
  reversing entries, linked by `reversed_by_entry_id`).
- Invoices post AR/revenue on issue, payments post cash/AR, expenses post expense/AP,
  payroll posts salary expense/payables.
- Reports (trial balance, AR aging, P&L by period) are SQL views; `billing` renders
  them to PDF/XLSX.

## Documents

Uploads never pass through the API process. The API issues a presigned S3 PUT URL,
the browser uploads directly, and a confirmation call records the key, size and MIME
type. Downloads are presigned GET URLs scoped by the same authorisation rules.
Locally, MinIO stands in for S3.

## Notifications

Writes that need an email add a row to `notifications` in the same transaction as the
business change (transactional outbox). `notify` polls with
`SELECT ... FOR UPDATE SKIP LOCKED`, sends through SMTP (Mailpit locally, Amazon SES
in production), retries with exponential backoff, and parks a row as `failed` after
eight attempts. Nothing is lost if the mail provider is down.

## Observability

- Structured JSON logs (`tracing`) with `request_id`, `user_id`, route, latency.
- `/metrics` Prometheus endpoint on every service (request counts, latency
  histograms, DB pool stats, outbox depth).
- `/healthz` (process up) and `/readyz` (database reachable, migrations current).
- Audit log queryable by admins and auditors through the API and UI.

## Data model

Eight migration files under `db/migrations`, applied by the API on start (`sqlx`).
Tables are grouped by prefix rather than schema so every service sees the same names:

- **org**: `departments`, `positions`, `employees`
- **identity**: `users`, `roles`, `permissions`, `role_permissions`, `user_roles`, `refresh_tokens`
- **hr**: `leave_types`, `leave_requests`, `leave_balances`, `shifts`, `attendance`, `employee_documents`
- **ops**: `customers`, `carriers`, `sites`, `vehicles`, `shipments`, `shipment_legs`, `shipment_events`, `shipment_documents`, `work_orders`, `inventory_items`
- **finance**: `accounts`, `fiscal_periods`, `journal_entries`, `journal_lines`, `invoices`, `invoice_lines`, `payments`, `vendors`, `vendor_bills`, `expenses`, `payroll_runs`, `payroll_items`
- **comms**: `threads`, `thread_participants`, `messages`, `support_tickets`
- **platform**: `audit_log`, `notifications`

`DOMAIN.md` describes every entity and rule; the SQL is the contract.

## AWS topology (Terraform)

```
Route 53 -> ALB (TLS) -> ECS Fargate services: web, api, billing, analytics, notify
                                   |                |            |
                          RDS PostgreSQL 16   ElastiCache   S3 (documents, pdfs)
                          (Multi-AZ, encrypted)  Redis 7      SES (outbound mail)
                          Secrets Manager (DB, JWT, SMTP)   CloudWatch logs + alarms
                          ECR (one repo per service)
```

Modules: `network`, `database`, `cache`, `storage`, `ecr`, `ecs`, `mail`, `secrets`,
`observability`. One environment directory per stage; `terraform plan` runs on every
pull request, `apply` runs from the deploy workflow behind a protected environment.

## CI/CD

`ci.yml` runs on every push and pull request, one job per service, all in parallel:
Rust (fmt, clippy with warnings denied, tests against a Postgres service container),
web (lint, typecheck, build, unit tests), billing (`./mvnw verify`), analytics (ruff,
pytest), tools (`go vet`, `go test`), Terraform (`fmt -check`, `validate`), and a
Docker build of every image. `deploy.yml` builds and pushes images to ECR on `main`,
then applies Terraform for the target environment.

## Security controls

- Argon2id hashing, JWT with short expiry, rotating refresh tokens with reuse detection
- Account lockout, per-IP and per-user rate limiting, forced password change
- Least-privilege database roles (`bowline_app` read/write, `bowline_ro` read-only for billing and analytics, `bowline_notify` outbox only)
- Input validation on every request body (`validator`), typed ids, no string-built SQL
- Append-only audit log, immutable ledger, closed-period lock
- Secrets from environment or AWS Secrets Manager only; `.env.example` documents every variable
- Security headers and strict CORS on the API; httpOnly cookie for the refresh token in the web app
- Dependency and container scanning in CI

## Local development

```
cp .env.example .env
docker compose up -d postgres redis mailpit minio
make api      # migrations run on start
make seed     # 260-person company, customers, shipments, ledger
make web      # http://localhost:3000, login as ceo@bowline.example / Bowline!2026
```

`scripts/smoke.sh` exercises the whole thing end to end: CEO logs in, broadcasts an
announcement, a dock worker opens a support ticket, an agent resolves it, a
coordinator books a shipment, an accountant issues the invoice, the ledger balances.
