# Database

PostgreSQL 16 is the single system of record. This directory is the contract every
service is written against.

```
migrations/   versioned SQL applied in order by the API (sqlx) or `make migrate`
init/         roles.sql: the three least-privilege roles created for local Docker
tests/        integrity.sql: rolled-back scenario proving the integrity rules
```

## Migrations

| File                        | Contents                                                                 |
|-----------------------------|--------------------------------------------------------------------------|
| `0001_extensions.sql`       | ltree, citext, pgcrypto, the shared `set_updated_at()` trigger           |
| `0002_org_identity.sql`     | departments, positions, employees (ltree path triggers), users, roles, permissions, refresh tokens |
| `0003_hr.sql`               | leave types, balances, requests (overlap exclusion), shifts, attendance, documents |
| `0004_ops.sql`              | customers, carriers, sites, vehicles, shipments, legs, events, documents, work orders, inventory |
| `0005_finance.sql`          | accounts, periods, journal (balance, immutability and closed-period triggers), invoices, payments, vendors, bills, expenses, payroll, report views |
| `0006_comms.sql`            | threads, participants, messages, support tickets                         |
| `0007_audit_outbox.sql`     | append-only audit log, notifications outbox, read-only grants            |
| `0008_reference_data.sql`   | 52 permissions, 14 roles, leave types, chart of accounts, fiscal periods (idempotent) |

Rules: never edit an applied migration; add a new numbered file. Statuses are
`text` with `check` constraints rather than enums so a new state is a one-line
constraint change. Every table with mutable rows has `updated_at` maintained by
trigger.

## Roles

| Role             | Used by                | Rights                                   |
|------------------|------------------------|------------------------------------------|
| `bowline_app`    | api (migrate, seed)    | owner of the schema; `CREATEDB` locally so the test suite can create throwaway databases |
| `bowline_ro`     | billing, analytics     | `SELECT` on every table and view          |
| `bowline_notify` | notify worker          | `SELECT`, `UPDATE` on `notifications` only |

## Integrity test

```
psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f db/tests/integrity.sql
```

Creates a five-person org and a few ledger rows inside a transaction, asserts the
eleven rules below, and rolls back. CI runs it after applying the migrations twice.

1. Paths are derived from the manager chain (depth of a fifth-level report is 5)
2. A second employee without a manager (a second CEO) is rejected
3. Reporting cycles are rejected, including through grandchildren
4. Re-parenting rewrites every descendant's path; subtree counts follow
5. `employees.path` cannot be written by hand
6. An unbalanced journal entry cannot be committed
7. A balanced entry is accepted; its lines and the entry are then immutable
8. Posting into a closed period is rejected
9. The trial balance sums to zero
10. Overlapping leave requests are rejected
11. The audit log accepts no updates or deletes
