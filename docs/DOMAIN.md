# Bowline domain model

This document is the human-readable contract behind `db/migrations`. When the two
disagree, the SQL wins and this file gets fixed.

## The company

The seed data models **Bowline Logistics**, a freight forwarder with about 260
employees on five sites (head office, sea-port depot, airport depot, road hub, bonded
warehouse). Money is in USD; the fiscal year is the calendar year.

### Levels (chain of command)

| Level | Name         | Examples                                                        | Typical roles               |
|------:|--------------|-----------------------------------------------------------------|-----------------------------|
| 1     | Chief        | Chief Executive Officer                                          | executive                   |
| 2     | C-suite      | COO, CFO, CHRO, CTO, Chief Commercial Officer                    | executive                   |
| 3     | Director     | Director of Sea Freight, Director of Finance, Director of IT     | director                    |
| 4     | Manager      | Warehouse Manager, Billing Manager, Fleet Manager, HR Manager    | manager, (+ hr_admin etc.)  |
| 5     | Supervisor   | Shift Supervisor, Dispatch Supervisor, Support Team Lead         | supervisor                  |
| 6     | Specialist   | Freight Coordinator, Accountant, Recruiter, Support Agent        | staff, accountant, agent    |
| 7     | Ground       | Driver, Forklift Operator, Dock Worker, Warehouse Handler        | field_worker                |

Every employee except the CEO has exactly one `manager_id`. The materialised
`path` (ltree) is the list of employee ids from the CEO down to the employee.

### Departments (seed)

```
Executive Office
Operations (COO)
  Sea Freight        Air Freight        Road and Last Mile
  Warehousing        Fleet and Drivers  Customs and Compliance
Finance (CFO)
  Accounting         Billing and AR     Payroll            Procurement and AP
People (CHRO)
  Talent Acquisition People Operations
Technology (CTO)
  Platform Engineering   Service Desk (the support desk everyone can write to)
Commercial (CCO)
  Sales               Customer Service
```

## Roles and permissions

Roles are bundles of permissions. A user may hold several roles (a Billing Manager is
`manager` + `finance_admin`). Permission keys follow `resource:action[:scope]`.

| Role            | Intended holders                         | Key permissions (in addition to the baseline)                                                                                                 |
|-----------------|------------------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------|
| baseline (all)  | every active user                        | `org:read`, `employees:read:self`, `leave:request`, `attendance:record:self`, `documents:read:self`, `tasks:read:self`, `tasks:update:self`, `expenses:submit`, `messages:send:chain`, `messages:send:department`, `tickets:create`, `shipments:read` |
| field_worker    | drivers, dock crews, handlers            | baseline only; UI shows tasks, shifts, inbox, support                                                                                          |
| staff           | coordinators, specialists                | `shipments:write`, `customers:read`                                                                                                             |
| supervisor      | level 5                                  | `employees:read:subtree`, `tasks:manage:subtree`, `shifts:manage:subtree`, `leave:approve:subtree`, `expenses:approve:subtree`, `messages:send:subtree` |
| manager         | level 4                                  | supervisor set + `employees:write:subtree`, `messages:broadcast:subtree`, `shipments:assign`                                                    |
| director        | level 3                                  | manager set + `employees:read:department`, `reports:read:department`                                                                            |
| executive       | levels 1 and 2                           | director set + `employees:read:all`, `messages:broadcast:company`, `reports:read:all`, `audit:read`, `payroll:read:all`                        |
| hr_admin        | People Operations                        | `employees:read:all`, `employees:write:all`, `leave:manage:all`, `documents:manage:all`, `payroll:prepare`, `users:manage`                      |
| accountant      | Accounting, Billing, Payroll specialists | `ledger:read`, `ledger:post`, `invoices:draft`, `invoices:issue`, `payments:record`, `vendors:manage`, `payroll:prepare`, `reports:read:all`     |
| finance_admin   | CFO, Director of Finance, Billing Mgr    | accountant set + `invoices:approve`, `expenses:approve:finance`, `payroll:approve`, `periods:close`                                              |
| dispatcher      | dispatch supervisors and coordinators    | `shipments:write`, `shipments:assign`, `fleet:manage`, `tasks:manage:subtree`, `customers:read`                                                 |
| support_agent   | Service Desk                             | `tickets:manage`, `tickets:read:all`, `messages:send:any` (replies inside tickets)                                                              |
| it_admin        | Platform Engineering leads               | `users:manage`, `roles:manage`, `audit:read`, `system:admin`                                                                                   |
| auditor         | external / internal audit                | `ledger:read`, `audit:read`, `employees:read:all`, `reports:read:all` (read only everywhere)                                                    |

### Scope resolution

Given a permission family such as `employees:read`, the effective scope is the widest
one the principal holds: `all` > `department` > `subtree` > `self`. Queries then add:

- `self`: `employee_id = :me`
- `subtree`: `path <@ :my_path`
- `department`: `department_id in (my department and its children)`
- `all`: no filter

Support tickets, shipments and invoices are not employee-scoped; they use their own
permission keys.

## Messaging

Thread kinds: `direct`, `announcement`, `ticket`.

| Sender wants to write to        | Allowed when                                                                 |
|---------------------------------|-------------------------------------------------------------------------------|
| the Service Desk                | always (`tickets:create`), creates or appends to a ticket thread             |
| their manager / a direct report | always (`messages:send:chain`)                                               |
| someone in the same department  | always (`messages:send:department`)                                          |
| anyone in their subtree         | `messages:send:subtree`                                                      |
| everyone in their subtree       | `messages:broadcast:subtree` (announcement)                                  |
| the whole company               | `messages:broadcast:company` (announcement)                                  |
| anyone at all                   | `messages:send:any` (support agents, executives)                             |

Every message creates an `email` notification per recipient in the outbox.

## Support desk

- Categories: `it`, `hr`, `payroll`, `operations`, `facilities`, `other`
- Priorities and SLA (time to first response): `urgent` 1h, `high` 4h, `normal` 24h, `low` 72h
- Lifecycle: `open` -> `triaged` -> `in_progress` -> `waiting_on_requester` -> `resolved` -> `closed`;
  a requester may reopen a resolved ticket within 7 days.
- Only support agents assign, triage and resolve; requesters can comment and close.

## HR

- **Leave**: types `annual` (20 days), `sick` (10), `unpaid`, `parental`. A request
  spans whole days, cannot overlap another approved request, and is routed to the
  direct manager. Approval deducts from `leave_balances`. HR admins can create, approve
  or cancel on anyone's behalf.
- **Shifts and attendance**: supervisors schedule shifts for their subtree; employees
  clock in and out; a shift is `late` if clock-in is more than 10 minutes after start.
- **Documents**: contracts, IDs, certificates and payslips are private to the employee
  and to HR admins; managers see only the document list of their subtree, not the files.
- **Lifecycle**: `active`, `on_leave`, `suspended`, `terminated`. Terminating an
  employee disables the user, revokes refresh tokens and re-parents their reports to
  their manager (done in the API in one transaction).

## Operations

- **Customer**: code, name, contacts, billing address, credit limit, account manager.
- **Shipment**: reference `BWL-YYYY-NNNNNN`, customer, mode (`sea`, `air`, `road`, `rail`),
  incoterm, origin and destination, cargo (pieces, weight, volume, hazardous), declared
  value, ETD/ETA, owner (coordinator), status.
- **Legs**: ordered segments with carrier, vehicle and driver, planned and actual times.
- **Events**: the tracking timeline (`booked`, `picked_up`, `departed`, `arrived`,
  `customs_hold`, `customs_cleared`, `out_for_delivery`, `delivered`, `exception`).
- **Work orders**: the ground-staff task list (`loading`, `unloading`, `pickup`,
  `delivery`, `inspection`, `inventory`); assigned by dispatchers and supervisors,
  updated by the worker from their phone.
- **Inventory**: items held at a site for a shipment, with bin location and
  received/released timestamps.
- **Delay risk**: the API asks `analytics` for a 0 to 1 delay-risk score whenever a
  shipment is booked or a leg changes; the score is stored on the shipment.

### Shipment state machine

```
draft -> booked -> picked_up -> in_transit -> customs -> out_for_delivery -> delivered
  any non-terminal state -> exception -> (back to the previous state) | cancelled
  draft | booked -> cancelled
```

## Finance

- **Chart of accounts** (seeded): 1000 Cash, 1100 Accounts Receivable, 1200 Prepaid,
  1500 Vehicles and Equipment, 2000 Accounts Payable, 2100 Salaries Payable,
  2200 Taxes Payable, 3000 Share Capital, 3100 Retained Earnings, 4000 Freight Revenue,
  4100 Warehousing Revenue, 4200 Customs Brokerage Revenue, 5000 Carrier Costs,
  5100 Salaries, 5200 Fuel, 5300 Warehouse Operations, 5400 Office and Admin,
  5500 Depreciation, 5600 Bad Debt.
- **Journal entry**: date, period, memo, source (`invoice`, `payment`, `expense`,
  `payroll`, `bill`, `manual`), lines. Balanced at commit by a deferred constraint
  trigger; lines are immutable; posting into a closed period fails.
- **Invoice**: `draft` -> `issued` -> `partially_paid` -> `paid`; `void` from `draft` or
  `issued` (void posts a reversing entry). Totals of 50,000 or more require
  `invoices:approve` before issue. Issuing posts `AR / Revenue`, asks `billing` to
  render the PDF and stores its key.
- **Payment**: posts `Cash / AR` and updates `amount_paid`; overpayment is rejected.
- **Expense claim**: `submitted` -> `manager_approved` -> `finance_approved` -> `paid`
  (posts `Expense / Cash`); `rejected` from any pre-paid state.
- **Vendor bill (AP)**: `received` -> `approved` -> `paid`; posts `Expense / AP` then `AP / Cash`.
- **Payroll**: a run per period, one item per active employee (gross, deductions,
  net); `draft` -> `approved` -> `posted` (posts `Salaries / Salaries Payable`).
- **Period close**: `finance_admin` closes a month; reopening needs `system:admin`.
- **Reports**: trial balance, AR aging (0-30, 31-60, 61-90, 90+), P&L by period, all as
  SQL views over the ledger.

## Audit

`audit_log(at, actor_user_id, actor_employee_id, action, entity_type, entity_id, before,
after, ip, request_id)`; written by the API inside the same transaction as the change.
Auditors and executives can query it by entity, actor or time window.

## Identifiers and conventions

- All primary keys are `uuid` (v4); business references are human-readable
  (`EMP-000123`, `BWL-2026-000123`, `INV-2026-000123`, `TKT-000123`).
- Timestamps are `timestamptz` in UTC; dates that mean "a calendar day" are `date`.
- Money is `numeric(14,2)` with a 3-letter currency code.
- Statuses are lower-case text with a `check` constraint, never Postgres enums, so
  adding a state is a migration on one constraint.
