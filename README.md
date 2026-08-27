# Bowline

**Freight operations and workforce management for a 260-person logistics company,
from the CEO to the dock crew: one chain of command, one ledger, one audit trail.**

[![CI](https://github.com/rhs2/bowline/actions/workflows/ci.yml/badge.svg)](https://github.com/rhs2/bowline/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
![Rust](https://img.shields.io/badge/Rust-1.98-000000?logo=rust)
![TypeScript](https://img.shields.io/badge/TypeScript-Next.js_15-3178C6?logo=typescript&logoColor=white)
![Java](https://img.shields.io/badge/Java-17_Spring_Boot_3-ED8B00?logo=openjdk&logoColor=white)
![Python](https://img.shields.io/badge/Python-3.12_FastAPI-3776AB?logo=python&logoColor=white)
![Go](https://img.shields.io/badge/Go-1.27-00ADD8?logo=go&logoColor=white)
![PostgreSQL](https://img.shields.io/badge/PostgreSQL-16-4169E1?logo=postgresql&logoColor=white)
![Terraform](https://img.shields.io/badge/Terraform-AWS-7B42BC?logo=terraform&logoColor=white)

**Project page:** [rhs2.github.io/bowline](https://rhs2.github.io/bowline/) ·
**Documentation:** [index](docs/README.md) · [Architecture](docs/ARCHITECTURE.md) · [Domain model](docs/DOMAIN.md) · [API contract](docs/API.md) · [Runbook](docs/RUNBOOK.md) · [Security](docs/SECURITY.md)

Bowline is the platform a mid-size freight forwarder runs on. It carries the company
hierarchy (levels 1 to 7, CEO to ground staff), identity and access, HR (leave, shifts,
attendance, documents), freight operations (customers, shipments, legs, tracking
events, work orders, inventory), double-entry finance (invoices, payments, expenses,
vendor bills, payroll, period close, reports), internal mail with company-wide
announcements, a support desk with SLAs, and an append-only audit log. It is the 2026
re-engineering of an internal platform first built in 2022 to 2023; the domain
carried over, everything else was rebuilt around a Rust core with the heavy or
specialised jobs in the language that suits them.

<!-- VERIFY:BEGIN -->
## Verified

Every number below came from running the thing, on PostgreSQL 16 with all six
services up. Nothing here is an estimate.

| Suite | Result |
|---|---|
| `api` (Rust) | **62 tests**: 27 unit, 35 integration over the real HTTP surface against a throwaway database per binary |
| `web` (Vitest) | **66 tests**, lint and `tsc --noEmit` clean, production build with no backend running |
| `tools` (Go) | **58 tests**, `go vet` clean, stable over repeated runs |
| `billing` (JUnit) | **45 tests**, `./mvnw -B verify` BUILD SUCCESS |
| `analytics` (pytest) | **35 tests**, `ruff check` and `ruff format --check` clean |
| `db` | **11 integrity rules** proven by a rolled-back SQL scenario, against both an empty and a seeded database |
| `infra` | **12 of 12** Terraform directories validate; a real plan resolves 187 production resources |
| **Total** | **266 automated tests**, plus a **33 step** end-to-end scenario |

The API answers **99 OpenAPI paths / 126 operations**, and `cargo clippy --all-targets -- -D warnings` is clean.

`scripts/smoke.sh` drives seven seeded roles through one working day and checks the
rules rather than the plumbing: a dock worker is refused when announcing to the
company but receives the CEO's announcement; they cannot open a thread with the CFO
but can message their own manager; a shipment jump straight to `delivered` is
refused while the legal chain succeeds; a work order moves only for its assignee;
overpayment and unbalanced entries are both rejected; issuing an invoice posts a
balanced journal entry and the trial balance still sums to zero; posting into a
closed period fails; and the audit trail is readable by the CEO and not by the dock
worker. **All 33 pass.**

```
$ make seed
260 employees, 22 departments, 300 shipments, 90 invoices, 180 journal entries
trial balance 0.00   employees without a manager 1   deepest reporting chain 6

$ ./scripts/smoke.sh
All 33 checks passed.
```

Roughly 22,100 lines of Rust, 14,100 of TypeScript, 4,900 of Go, 4,700 of Terraform,
3,600 of Java, 2,500 of Python and 1,150 of SQL, with 4,000 lines of documentation.
<!-- VERIFY:END -->

## What makes it more than a CRUD app

- **The org chart is a data structure, not a picture.** Every employee's reporting
  path is a PostgreSQL `ltree` maintained by triggers: exactly one CEO, cycles are
  rejected, re-parenting a manager rewrites their whole subtree in one statement.
  "Everyone below me" is a GiST index lookup, and every permission that says
  `:subtree`, `:department` or `:all` is evaluated against that same path.
- **Money cannot go wrong quietly.** The ledger is double entry; a deferred
  constraint trigger refuses to commit an unbalanced entry, journal lines are
  immutable (corrections are reversing entries), and posting into a closed fiscal
  period fails at the database, not just in the API.
- **Who can talk to whom is a rule, not a habit.** A dock worker can write to the
  support desk, their supervisor and their own department; a manager can reach
  their subtree; only executives can address the whole company. The recipient picker
  in the UI and the API enforce the same table.
- **Every write is attributable.** Mutations carry the acting user, request id and a
  before/after snapshot into an append-only audit log, in the same transaction.
- **Nothing is lost when the mail provider is down.** Emails are rows in a
  transactional outbox; a Go worker claims them with `SKIP LOCKED`, retries with
  backoff, and parks failures for inspection.

## Architecture

```
                       +------------------+
   browser  ---------> |  web  (Next.js)  |  BFF: httpOnly cookies, /api/proxy
                       +--------+---------+
                                | HTTPS /api/v1  (JWT bearer)
                                v
   bowctl (Go) ------> +------------------+      +--------------------+
                       |  api  (Rust)     |----->| billing  (Java)    |  invoice PDFs,
                       |  Axum + SQLx     |      | Spring Boot        |  statements, AR aging xlsx
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
                       | notify (Go)      |  SES in production, Mailpit locally
                       +------------------+
```

| Service     | Language / framework                 | Owns                                                                 |
|-------------|--------------------------------------|----------------------------------------------------------------------|
| `api`       | Rust 1.98, Axum 0.7, SQLx 0.8, Tokio | All business rules, authorisation, migrations, OpenAPI, seed data    |
| `web`       | TypeScript, Next.js 15, Tailwind     | Role-aware UI: dashboards, org chart, inbox, support, HR, ops, finance, admin |
| `billing`   | Java 17, Spring Boot 3, OpenPDF, POI | Invoice PDFs, customer statements, AR aging spreadsheets             |
| `analytics` | Python 3.12, FastAPI, scikit-learn   | Shipment delay-risk scoring, weekly volume forecasts                 |
| `tools`     | Go 1.27                              | `notify` outbox worker, `bowctl` operator CLI                        |
| `infra`     | Terraform, GitHub Actions            | AWS (ECS Fargate, RDS, ElastiCache, S3, SES), CI, build and deploy   |
| `db`        | SQL                                  | Versioned migrations, reference data, integrity triggers and tests   |

The API is the only writer of business data. `billing` and `analytics` read through
a read-only database role and are called by the API over HTTP; `notify` may touch
only the outbox table. Full detail in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## The company it models

Seeded as **Bowline Logistics**: about 260 people across Executive, Operations (sea,
air, road, warehousing, fleet, customs), Finance (accounting, billing, payroll,
procurement), People, Technology (platform, service desk) and Commercial, on five
sites. Fourteen roles bundle 52 permissions; approval chains (leave, expenses,
invoices above a threshold, payroll, period close) follow the reporting line.
See [docs/DOMAIN.md](docs/DOMAIN.md) for the role table and every workflow.

## Quickstart

```bash
cp .env.example .env
make up            # Postgres 16, Redis 7, Mailpit, MinIO
make migrate       # applies db/migrations (the API also does this on start)
make seed          # 260 employees, customers, shipments, invoices, a balanced ledger
make api           # http://localhost:8080  (Swagger UI at /docs)
make web           # http://localhost:3000
```

Sign in as `ceo@bowline.example` / `Bowline!2026` (or `driver@`, `dock.worker@`,
`support.agent@`, `accountant@`, `cfo@`, `hr.admin@`, all `@bowline.example`) to see
the platform from different levels of the hierarchy. Mail sent by the platform lands
in Mailpit at http://localhost:8025.

`make test` runs every suite (Rust integration tests against Postgres, Vitest, JUnit,
pytest, Go). `make smoke` walks an end-to-end scenario against the running stack:
the CEO announces to the company, a dock worker opens a ticket that an agent
resolves, a dispatcher books a shipment and a driver completes the work order, an
accountant issues and collects the invoice, the trial balance stays at zero, and a
post into a closed period is refused.

## Repository map

```
api/            Rust API: src/{auth,org,hr,ops,finance,comms,support,admin}, bins: bowline-api, migrate, seed
web/            Next.js app: app/(app)/{dashboard,org,people,inbox,support,hr,ops,finance,admin}, lib/api.ts
billing/        Spring Boot: render/invoice, statements, reports/ar-aging.xlsx
analytics/      FastAPI: score/delay-risk, forecast/volume, train.py
tools/          Go: cmd/notify (outbox worker), cmd/bowctl (CLI), internal/outbox
db/             migrations/0001..0008, init/roles.sql, tests/integrity.sql
infra/          terraform/{modules,environments}, deployment notes
docs/           ARCHITECTURE, DOMAIN, API, RUNBOOK, SECURITY, project page
scripts/        dev-up.sh, smoke.sh, leak_scan.sh
.github/        ci.yml (one job per service) and deploy.yml (ECR + Terraform)
internal/       working notes, excluded from the repository
```

## Engineering notes

- **Migrations are the contract.** Eight SQL files under `db/migrations`; the
  integrity rules have their own test (`db/tests/integrity.sql`) that CI runs after
  applying the migrations twice, proving reference data is idempotent.
- **Tests run against real Postgres.** The Rust suite creates a throwaway database
  per run, migrates it, and exercises login lockout, refresh-token reuse detection,
  scope filtering, re-parenting, state machines, ledger posting and messaging rules
  through the HTTP layer.
- **Twelve-factor everywhere.** Every variable is documented in `.env.example`;
  production values come from AWS Secrets Manager through the ECS task definitions.
- **One pipeline.** `ci.yml` lints, tests and builds every service in parallel;
  `deploy.yml` assumes an OIDC role, pushes images to ECR, applies Terraform behind a
  protected environment and runs migrations as a one-off task.

## Publishing safely

Everything in this repository is meant to be public, so the boundary is enforced
rather than remembered.

`.gitignore` is ordered by consequence: secrets and real configuration first,
then Terraform state (which contains every resolved endpoint and generated
password), then `internal/`, then data, then the ordinary build output. The one
environment file that ships is `.env.example`, and it holds documented
placeholders.

`./scripts/leak_scan.sh` scans **what git would publish**, not the working tree,
because a secret in an ignored file is not a leak and the same secret in a tracked
file is. It looks for private keys, cloud and service tokens, JWTs, real AWS
account ids and ARNs, addresses outside the reserved test domains, credential
literals, and files that should never be tracked. The terms that identify a real
person or company are not listed in the script itself, since writing them there
would publish exactly what the check exists to catch; they load from
`scripts/private_patterns.txt`, which is excluded.

It runs three ways: by hand, from `.githooks/pre-commit` (enable once with
`git config core.hooksPath .githooks`), and in CI, where it is joined by a
full-history scan so that something committed and later deleted is still caught.

The scanner is tested the way any other guard should be: planting each kind of
secret and confirming it fails, then removing them and confirming it passes.

## License

MIT. The company, people, customers and figures in the seed data are fictional,
generated by `make seed` from a fixed random seed.
