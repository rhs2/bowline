# Bowline API

The Rust core of the Bowline platform: identity and access, the organisation chart,
HR, freight operations, double-entry finance, internal messaging, the support desk
and the audit trail. It is the only writer of business data. `billing` and
`analytics` read the same database through a read-only role and are called by this
service over HTTP; `notify` drains the `notifications` outbox.

Axum 0.7 on Tokio, SQLx 0.8 against PostgreSQL 16, no ORM and no string built SQL.
Every route lives under `/api/v1`; the OpenAPI document is generated from the
handlers with `utoipa` and served at `/api-docs/openapi.json`, with a browsable copy
at `/docs`.

## Running it locally

```bash
# from the repository root
cp .env.example .env
docker compose up -d postgres redis mailpit minio minio-init

cd api
cargo run --bin migrate             # apply db/migrations
cargo run --bin seed                # load the 260 person demo company
cargo run                           # serve on http://localhost:8080
```

`make up`, `make migrate`, `make seed` and `make api` from the repository root do
the same thing. Redis, MinIO and Mailpit are optional: without Redis the principal
cache and the rate limiter fall back to process memory, and the service keeps
serving.

Useful endpoints once it is up:

| Path                     | What it is                                            |
|--------------------------|-------------------------------------------------------|
| `/healthz`               | process liveness, no database access                  |
| `/readyz`                | database reachable and no migrations pending          |
| `/metrics`               | Prometheus text exposition, including outbox depth    |
| `/docs`                  | API reference rendered from the OpenAPI document      |
| `/api/v1/auth/login`     | `{email, password}` for an access and refresh token   |

### Binaries

The crate builds three binaries, and the container image carries all three.

| Binary        | Purpose                                                                     |
|---------------|-----------------------------------------------------------------------------|
| `bowline-api` | the service                                                                 |
| `migrate`     | applies `db/migrations` and exits, for a deploy step that runs before rollout |
| `seed`        | loads the demo company                                                       |

`seed` is deterministic: the same `SEED_RANDOM_SEED` rebuilds the same company down
to the primary keys. It is also idempotent, so running it twice is safe: if
`ceo@bowline.example` already exists it prints a note and exits 0. `seed --reset`
truncates every business table first (permissions, roles, role permissions, leave
types, the chart of accounts and the fiscal periods are reference data and are kept
or rebuilt) and then loads a fresh company. It writes about 260 employees with
their users and roles, five sites, six carriers, 25 vehicles, 40 customers, twelve
vendors, 300 shipments with legs, tracking events and documents, work orders,
inventory, leave, shifts, attendance, invoices with payments and their journal
entries, expenses, vendor bills, one posted payroll run, support tickets,
announcements and the notification outbox rows that go with them. Seeding takes a
few seconds; it builds the company in memory and writes it in batched inserts
inside one transaction.

## Configuration

Everything is read from the environment on start. A `.env` in the working directory
or its parent is loaded first as a development convenience, and a value already in
the environment always wins. `../.env.example` documents every variable in the
platform; the ones this service reads are:

| Variable                      | Default                  | Notes                                                     |
|-------------------------------|--------------------------|-----------------------------------------------------------|
| `DATABASE_URL`                | required                 | PostgreSQL 16, the read/write application role            |
| `DATABASE_MAX_CONNECTIONS`    | `20`                     | pool size                                                 |
| `DATABASE_MIGRATE_ON_START`   | `1`                      | `0` makes the process refuse to start with work pending   |
| `REDIS_URL`                   | none                     | principal cache and rate limiting; optional               |
| `API_BIND`                    | `0.0.0.0:8080`           | listen address                                            |
| `API_PUBLIC_URL`              | `http://localhost:8080`  | used in links the service hands out                       |
| `API_CORS_ORIGINS`            | empty                    | comma separated allow list for the browser app            |
| `JWT_SECRET`                  | required, 32 chars up    | HS256 signing key for access tokens                       |
| `JWT_ISSUER`                  | `bowline`                | `iss` claim                                               |
| `ACCESS_TOKEN_TTL_SECONDS`    | `900`                    | 15 minutes                                                |
| `REFRESH_TOKEN_TTL_SECONDS`   | `2592000`                | 30 days, rotated on every use                             |
| `LOGIN_MAX_FAILURES`          | `5`                      | failures before the account locks                         |
| `LOGIN_LOCKOUT_SECONDS`       | `900`                    | how long the lock lasts                                   |
| `RATE_LIMIT_PER_MINUTE`       | `300`                    | per user, per IP for anonymous routes                     |
| `INVOICE_APPROVAL_THRESHOLD`  | `50000`                  | totals at or above this need `invoices:approve`           |
| `BILLING_URL`                 | `http://localhost:8081`  | invoice PDFs and AR spreadsheets                          |
| `ANALYTICS_URL`               | `http://localhost:8082`  | delay risk scoring                                        |
| `INTERNAL_SERVICE_TOKEN`      | empty                    | sent as `X-Internal-Token` to those two services          |
| `LOG_FORMAT`                  | `pretty`                 | `json` in production                                      |
| `RUST_LOG`                    | `info,bowline_api=debug` | tracing filter                                            |
| `S3_ENDPOINT`                 | empty                    | set for MinIO, leave empty for real S3                    |
| `S3_REGION`                   | `us-east-1`              |                                                           |
| `S3_BUCKET_DOCUMENTS`         | `bowline-documents`      | employee and shipment documents                           |
| `S3_BUCKET_PDFS`              | `bowline-pdfs`           | rendered invoices and statements                          |
| `S3_ACCESS_KEY_ID`            | empty                    |                                                           |
| `S3_SECRET_ACCESS_KEY`        | empty                    |                                                           |
| `S3_FORCE_PATH_STYLE`         | `1`                      | required by MinIO, `0` for AWS                            |
| `PRESIGN_TTL_SECONDS`         | `900`                    | lifetime of presigned upload and download URLs            |
| `SEED_PASSWORD`               | `Bowline!2026`           | password given to every seeded account                    |
| `SEED_SKIP_PASSWORD_CHANGE`   | `1`                      | `0` forces a password change on first login               |
| `SEED_RANDOM_SEED`            | `42`                     | changes the generated company, not its shape              |

## Well known logins

Every seeded account uses `SEED_PASSWORD`. Ordinary staff sign in as
`firstname.lastname@bowline.example`; these seventeen accounts always exist and are
what the smoke test, the UI walkthrough and the screenshots use.

| Login                               | Position                       | Roles beyond `baseline`      |
|-------------------------------------|--------------------------------|------------------------------|
| `ceo@bowline.example`               | Chief Executive Officer        | `executive`                  |
| `coo@bowline.example`               | Chief Operating Officer        | `executive`                  |
| `cfo@bowline.example`               | Chief Financial Officer        | `executive`, `finance_admin` |
| `chro@bowline.example`              | Chief Human Resources Officer  | `executive`                  |
| `cto@bowline.example`               | Chief Technology Officer       | `executive`                  |
| `cco@bowline.example`               | Chief Commercial Officer       | `executive`                  |
| `director.finance@bowline.example`  | Director of Finance            | `director`, `finance_admin`  |
| `manager.billing@bowline.example`   | Billing Manager                | `manager`, `finance_admin`   |
| `manager.warehouse@bowline.example` | Warehouse Manager              | `manager`                    |
| `supervisor.dock@bowline.example`   | Dock Supervisor                | `supervisor`                 |
| `dispatcher@bowline.example`        | Dispatch Coordinator           | `staff`, `dispatcher`        |
| `accountant@bowline.example`        | Accountant                     | `staff`, `accountant`        |
| `hr.admin@bowline.example`          | People Operations Specialist   | `staff`, `hr_admin`          |
| `support.agent@bowline.example`     | Support Agent                  | `staff`, `support_agent`     |
| `it.admin@bowline.example`          | Platform Engineering Manager   | `manager`, `it_admin`        |
| `driver@bowline.example`            | Driver                         | `field_worker`               |
| `dock.worker@bowline.example`       | Dock Worker                    | `field_worker`               |

## Module map

```
src/
  main.rs        process start: config, pool, migrations, router, graceful shutdown
  lib.rs         the crate root, so integration tests and the binaries share one build
  config.rs      every environment variable, parsed and validated once
  db.rs          the pool and the embedded migration set (sqlx::migrate!("../db/migrations"))
  state.rs       AppState: pool, config, principal cache, rate limiter, service clients
  error.rs       ApiError and RFC 7807 problem responses with the stable codes
  scope.rs       hierarchy scopes turned into SQL predicates
  audit.rs       append only audit_log writer, called inside the caller's transaction
  outbox.rs      one notifications row per recipient, written in the same transaction
  telemetry.rs   tracing setup, JSON in production
  health.rs      /healthz, /readyz, /metrics
  openapi.rs     the assembled OpenAPI document

  auth/          login, refresh with rotation and reuse detection, logout, password
                 change, /auth/me; Argon2id hashing, JWT issue and verify, the
                 Principal (user, employee, roles, permissions, reporting path) and
                 its cache, and the Actor a handler acts as
  org/           departments, positions, employees, the org tree, re-parenting and
                 termination (one transaction: reports move up, user disabled, tokens
                 revoked)
  hr/            leave requests, balances and approvals, shifts, attendance,
                 employee documents
  ops/           customers, carriers, sites, vehicles, shipments with their legs,
                 tracking events and documents, work orders, inventory
  finance/       the ledger, invoices, payments, vendor bills, expense claims,
                 payroll and the report views
  comms/         threads, messages, announcements and the rules about who may write
                 to whom
  support/       tickets: create, triage, assign, resolve, close, reopen
  admin/         users, roles and the audit log
  dashboard.rs   one role aware summary: each block appears only when the caller
                 holds the permission behind it
  http/          router assembly and middleware (request id, rate limit, tracing,
                 CORS, compression, timeouts, security headers), typed extractors
                 and list pagination
  clients/       outbound: billing (PDFs, spreadsheets), analytics (delay risk), S3
                 (presigned upload and download URLs)
  bin/migrate.rs applies migrations and exits
  bin/seed.rs    builds and writes the demo company
```

## Authorisation

Authentication produces a `Principal`: the user, their employee record, their roles,
the union of their permission keys, their department and their reporting path.

Permission keys read `resource:action[:scope]`. The scope suffix decides how much of
the company an action reaches, and the widest one the principal holds wins:
`all` > `department` > `subtree` > `self`. A handler asks for a family such as
`employees:read`, gets back the effective `Scope`, and the query is written against
it:

| Scope        | Predicate                                             |
|--------------|-------------------------------------------------------|
| `self`       | `employee_id = :me`                                   |
| `subtree`    | `path <@ :my_path`                                    |
| `department` | `department_id = any(:my department and its children)` |
| `all`        | no filter                                             |

`employees.path` is a PostgreSQL `ltree` holding the chain of employee ids from the
CEO down to that employee. It is maintained by triggers: inserting or re-parenting
an employee recomputes the path and rewrites the whole subtree in one statement,
cycles are rejected, and the column cannot be written by hand. "Everyone below me"
is therefore a GiST index lookup rather than a recursive query, and there is no
second copy of the org chart to keep in step.

Two consequences worth knowing when writing a handler: there are no unscoped list
endpoints, and a detail route for a row outside the caller's scope answers 404
rather than 403, so the API never confirms that a record exists to someone who may
not see it.

## The ledger

Invoices, payments, expense claims, vendor bills and payroll are all just sources of
journal entries. Posting rules:

* **One transaction per entry.** The balance check is a *deferred* constraint
  trigger, so it runs at commit, once every line of the entry has been written. An
  entry and its lines must go in together; the seeder and every handler do exactly
  that.
* **Nothing is edited after the fact.** Journal lines are immutable and entries
  cannot be deleted. A correction is a reversing entry, linked both ways through
  `reverses_entry_id` and `reversed_by_entry_id`. Voiding an issued invoice posts
  the mirror image of the original entry rather than removing it.
* **Closed periods are closed.** Every entry belongs to a `fiscal_period` and must
  fall inside its dates; a trigger rejects anything posted into a closed month.
  Closing needs `periods:close`, reopening needs `system:admin`.
* **What posts where.** Issuing an invoice posts accounts receivable against
  revenue plus the tax liability; a payment posts cash against receivables and moves
  `amount_paid`, with overpayment rejected; an approved expense claim posts the
  expense against cash when it is paid; a vendor bill posts the expense against
  accounts payable, then payables against cash on payment; a posted payroll run
  posts gross salaries against net pay and payroll taxes.
* **Reports are views.** `trial_balance`, `ar_aging` and `profit_and_loss` are SQL
  views over the ledger, so they cannot drift from it. `select sum(balance) from
  trial_balance` is 0.00 on any healthy database, seeded or not.

## Tests

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

Three layers, all run by the `api` job in `.github/workflows/ci.yml` against a
Postgres service container:

1. **Unit tests** next to the code they cover, in `#[cfg(test)] mod tests`. These are
   the pure pieces: password hashing and policy, JWT round trips, scope resolution,
   state machine transitions, money arithmetic, reference formatting.
2. **Integration tests** in `api/tests`, driving the assembled router with `tower`'s
   `oneshot` against a real database, each test inside a transaction that is rolled
   back. This layer takes the paths that only exist end to end: login, refresh
   rotation and reuse detection, scoped list endpoints seen through different
   principals, approval workflows, and the problem document shape of every error.
3. **Database integrity tests** in `db/tests/integrity.sql`, which assert the rules
   the schema enforces on its own: one employee without a manager, path cascades and
   cycle rejection, unbalanced entries refused, immutable journal lines, closed
   period locking, overlapping leave refused, append only audit log.

`scripts/smoke.sh` runs the whole stack end to end against a seeded database: the
CEO logs in and broadcasts, a dock worker opens a ticket, an agent resolves it, a
coordinator books a shipment, an accountant issues the invoice, and the ledger
balances.

## Container image

The build context is the repository root, because the migration set is embedded at
compile time from `db/migrations`, which sits outside this directory:

```bash
docker build -f api/Dockerfile -t bowline/api:dev .
docker run --rm -p 8080:8080 --env-file .env bowline/api:dev
```

The build stage caches dependencies before the sources are copied; the runtime stage
is `debian:bookworm-slim` with CA certificates, a non-root user, all three binaries
on the path, and a healthcheck against `/healthz`. To run a task instead of the
service, override the entrypoint: `docker run --rm --env-file .env
--entrypoint /usr/local/bin/migrate bowline/api:dev`.

`api/.dockerignore` holds the ignore list for this image. Docker only reads an
ignore file from the context root or from `<dockerfile>.dockerignore` beside the
Dockerfile, so for a root context build keep the repository root ignore file in step
with it.
