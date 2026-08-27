# Bowline API contract (v1)

Base path `/api/v1`. JSON in, JSON out. Interactive docs at `/docs` (Swagger UI from
the `utoipa` OpenAPI document at `/api-docs/openapi.json`).

## Conventions

- **Auth**: `Authorization: Bearer <access token>` on every route except `/auth/login`,
  `/auth/refresh`, `/healthz`, `/readyz`, `/metrics`.
- **Ids**: UUID strings. Business references (`EMP-000123`, `BWL-2026-000123`,
  `INV-2026-000123`, `TKT-000123`) are display-only; routes take UUIDs.
- **Timestamps**: RFC 3339 UTC. Dates: `YYYY-MM-DD`. Money: decimal strings
  (`"1250.00"`) with a `currency` field.
- **Lists**: `?page=1&per_page=25` (max 100), optional `?sort=field&order=asc|desc`,
  free-text `?q=`. Response envelope:
  `{"items": [...], "page": 1, "per_page": 25, "total": 137}`.
- **Errors**: RFC 7807 `application/problem+json`:
  `{"type":"about:blank","title":"Forbidden","status":403,"detail":"...","code":"forbidden","request_id":"..."}`.
  Codes: `validation_failed` (422, with `errors: [{field, message}]`), `unauthorized` (401),
  `forbidden` (403), `not_found` (404), `conflict` (409), `invalid_transition` (409),
  `locked` (423), `rate_limited` (429), `internal` (500).
- **Scoping**: list endpoints return only what the caller may see; detail endpoints
  return 404 (not 403) for rows outside the caller's scope.

## Auth

| Method | Path                     | Body / notes                                                                       |
|--------|--------------------------|------------------------------------------------------------------------------------|
| POST   | `/auth/login`            | `{email, password}` -> `{access_token, refresh_token, expires_in, must_change_password}` |
| POST   | `/auth/refresh`          | `{refresh_token}` -> same shape; old token revoked; reuse revokes the family        |
| POST   | `/auth/logout`           | `{refresh_token}` -> 204                                                            |
| POST   | `/auth/change-password`  | `{current_password, new_password}` -> 204; bumps `token_version`                    |
| GET    | `/auth/me`               | `{user:{id,email,must_change_password}, employee:{...}, roles:[key], permissions:[key], chain:[{id,name,title,level}]}` |

## Organisation

| Method | Path                               | Permission                            | Notes                                                       |
|--------|------------------------------------|---------------------------------------|-------------------------------------------------------------|
| GET    | `/org/tree`                        | `org:read`                            | whole tree: `{id,name,title,level,department,children:[]}` (names and titles only) |
| GET    | `/org/departments`                 | `org:read`                            | flat list with `parent_id`, `head` (employee) and `headcount` |
| GET    | `/org/positions`                   | `org:read`                            |                                                             |
| GET    | `/employees`                       | `employees:read:*` (scoped)           | filters: `department_id`, `status`, `level`, `q`             |
| GET    | `/employees/{id}`                  | scoped                                | full record incl. `manager`, `direct_reports_count`          |
| POST   | `/employees`                       | `employees:write:all`                 | creates employee + user (temporary password returned once)   |
| PATCH  | `/employees/{id}`                  | `employees:write:subtree` or `:all`   | fields incl. `manager_id` (re-parent), `position_id`, `status` |
| POST   | `/employees/{id}/terminate`        | `employees:write:all`                 | `{termination_date, reassign_reports_to?}`; disables user, revokes tokens |
| GET    | `/employees/{id}/reports`          | scoped                                | direct reports                                               |
| GET    | `/employees/{id}/chain`            | `org:read`                            | chain of command up to the CEO                               |

## HR

| Method | Path                                  | Permission                          | Notes                                                         |
|--------|---------------------------------------|-------------------------------------|---------------------------------------------------------------|
| GET    | `/hr/leave/types`                     | any                                 |                                                               |
| GET    | `/hr/leave/balances?employee_id=`     | self / subtree / all                | current year                                                  |
| GET    | `/hr/leave/requests`                  | scoped; `?status=&pending_for_me=1` | approvers use `pending_for_me`                                |
| POST   | `/hr/leave/requests`                  | `leave:request`                     | `{type_key,start_date,end_date,reason}`; routes to manager    |
| POST   | `/hr/leave/requests/{id}/approve`     | `leave:approve:subtree` / `leave:manage:all` | `{note?}`                                             |
| POST   | `/hr/leave/requests/{id}/reject`      | same                                | `{note}`                                                      |
| POST   | `/hr/leave/requests/{id}/cancel`      | owner or `leave:manage:all`         |                                                               |
| GET    | `/hr/shifts?employee_id=&from=&to=`   | self / subtree                      |                                                               |
| POST   | `/hr/shifts`                          | `shifts:manage:subtree`             | `{employee_id,site,starts_at,ends_at,role_on_shift}`          |
| POST   | `/hr/attendance/clock-in`             | `attendance:record:self`            | `{shift_id?}` -> record; `late` computed                      |
| POST   | `/hr/attendance/clock-out`            | `attendance:record:self`            |                                                               |
| GET    | `/hr/attendance?employee_id=&from=&to=` | self / subtree                    |                                                               |
| GET    | `/hr/documents?employee_id=`          | self / `documents:manage:all`       | managers see the list for their subtree, not download URLs    |
| POST   | `/hr/documents/presign`               | self / `documents:manage:all`       | `{employee_id,kind,title,mime_type,size_bytes}` -> `{upload_url,s3_key}` |
| POST   | `/hr/documents`                       | same                                | confirms `{s3_key,...}` after upload                           |
| GET    | `/hr/documents/{id}/download`         | self / `documents:manage:all`       | -> `{url}` presigned                                          |

## Operations

| Method | Path                                    | Permission                 | Notes                                                             |
|--------|-----------------------------------------|----------------------------|-------------------------------------------------------------------|
| GET/POST | `/ops/customers`, GET/PATCH `/ops/customers/{id}` | `customers:read` / `customers:manage` |                                                  |
| GET/POST | `/ops/carriers`, `/ops/sites`, `/ops/vehicles` | `shipments:read` / `fleet:manage` |                                                          |
| GET    | `/ops/shipments`                        | `shipments:read`           | filters: `status`, `customer_id`, `mode`, `owner_id`, `q`          |
| POST   | `/ops/shipments`                        | `shipments:write`          | creates `draft`; reference assigned; delay risk scored async      |
| GET    | `/ops/shipments/{id}`                   | `shipments:read`           | with `legs`, `events`, `documents`, `work_orders`, `invoice`      |
| PATCH  | `/ops/shipments/{id}`                   | `shipments:write`          | cargo, dates, owner                                               |
| POST   | `/ops/shipments/{id}/transition`        | `shipments:write`          | `{to, note?, location?}`; enforces the state machine              |
| POST   | `/ops/shipments/{id}/legs`              | `shipments:assign`         | `{seq,mode,carrier_id,vehicle_id,driver_id,from,to,planned_*}`    |
| PATCH  | `/ops/shipments/{id}/legs/{leg_id}`     | `shipments:assign`         | actual times, status                                              |
| POST   | `/ops/shipments/{id}/events`            | `shipments:write` or assigned driver | `{event_type,location,note}`                          |
| POST   | `/ops/shipments/{id}/documents/presign`, POST `/documents`, GET `/documents/{doc_id}/download` | `shipments:write` / `shipments:read` | as HR docs |
| GET    | `/ops/work-orders`                      | `tasks:read:self` (mine) / `tasks:manage:subtree` | `?mine=1&status=`                          |
| POST   | `/ops/work-orders`                      | `tasks:manage:subtree` or `shipments:assign` | `{shipment_id?,site_id?,kind,title,instructions,assigned_to,due_at}` |
| POST   | `/ops/work-orders/{id}/status`          | assignee (`tasks:update:self`) or manager | `{status, notes?}`                                   |
| GET/POST | `/ops/inventory?site_id=`             | `shipments:read` / `shipments:write` |                                                         |

## Finance

| Method | Path                                   | Permission                          | Notes                                                        |
|--------|----------------------------------------|-------------------------------------|--------------------------------------------------------------|
| GET    | `/finance/accounts`                    | `ledger:read`                       |                                                              |
| GET    | `/finance/periods`                     | `ledger:read`                       |                                                              |
| POST   | `/finance/periods/{id}/close`          | `periods:close`                     | `/reopen` needs `system:admin`                               |
| GET    | `/finance/journal?period_id=&account=` | `ledger:read`                       | entries with lines                                           |
| POST   | `/finance/journal`                     | `ledger:post`                       | `{entry_date, memo, lines:[{account_code, debit, credit, description}]}` |
| POST   | `/finance/journal/{id}/reverse`        | `ledger:post`                       | posts the mirror entry, links both                           |
| GET    | `/finance/invoices`                    | `ledger:read` or `customers:read`   | filters `status`, `customer_id`, `overdue=1`                 |
| POST   | `/finance/invoices`                    | `invoices:draft`                    | `{customer_id, shipment_id?, currency, due_days, lines:[{description,quantity,unit_price,tax_rate}]}` |
| PATCH  | `/finance/invoices/{id}`               | `invoices:draft`                    | drafts only                                                  |
| POST   | `/finance/invoices/{id}/submit`        | `invoices:draft`                    | draft -> pending_approval (if >= threshold) else approved     |
| POST   | `/finance/invoices/{id}/approve`       | `invoices:approve`                  | pending_approval -> approved                                 |
| POST   | `/finance/invoices/{id}/issue`         | `invoices:issue`                    | approved -> issued: posts AR/Revenue, renders PDF via billing |
| POST   | `/finance/invoices/{id}/void`          | `invoices:approve`                  | reversing entry if issued                                    |
| GET    | `/finance/invoices/{id}/pdf`           | `ledger:read` or `customers:read`   | -> `{url}`                                                   |
| POST   | `/finance/payments`                    | `payments:record`                   | `{invoice_id, received_on, amount, method, reference}`       |
| GET/POST | `/finance/vendors`, `/finance/bills`, POST `/finance/bills/{id}/approve`, `/pay` | `vendors:manage`, pay needs `expenses:approve:finance` | |
| GET    | `/finance/expenses`                    | mine / `expenses:approve:*`         | `?pending_for_me=1`                                          |
| POST   | `/finance/expenses`                    | `expenses:submit`                   | `{category, amount, currency, incurred_on, description, receipt_s3_key?}` |
| POST   | `/finance/expenses/{id}/approve`       | manager then finance                | step decided by current status and caller's permission       |
| POST   | `/finance/expenses/{id}/reject`        | same                                | `{note}`                                                     |
| POST   | `/finance/expenses/{id}/pay`           | `expenses:approve:finance`          | posts Expense / Cash                                         |
| GET    | `/finance/payroll/runs`                | `payroll:read:all` / `payroll:prepare` |                                                           |
| POST   | `/finance/payroll/runs`                | `payroll:prepare`                   | `{period_id}`; one item per active employee from base_salary |
| POST   | `/finance/payroll/runs/{id}/approve`, `/post` | `payroll:approve`            | post writes Salaries / Salaries Payable                      |
| GET    | `/finance/reports/trial-balance`       | `ledger:read`                       |                                                              |
| GET    | `/finance/reports/ar-aging`            | `ledger:read`                       | rows + bucket totals; `?format=xlsx` proxies to billing      |
| GET    | `/finance/reports/pnl?year=&month=`    | `ledger:read`                       |                                                              |

## Communications

| Method | Path                                  | Permission                         | Notes                                                          |
|--------|---------------------------------------|------------------------------------|----------------------------------------------------------------|
| GET    | `/comms/recipients?q=`                | any                                | people the caller is allowed to message (rules in DOMAIN.md)   |
| GET    | `/comms/threads?kind=&unread=1`       | participant                        | inbox; each row has `unread_count`, `last_message`             |
| GET    | `/comms/threads/{id}`                 | participant                        | messages + participants; marks read                            |
| POST   | `/comms/threads`                      | messaging rules                    | `{recipient_ids:[...], subject, body}` -> direct thread         |
| POST   | `/comms/threads/{id}/messages`        | participant                        | `{body, importance?}`                                          |
| POST   | `/comms/announcements`                | `messages:broadcast:*`             | `{scope: "company"|"department"|"subtree", ref?, subject, body}` |
| POST   | `/comms/threads/{id}/archive`         | participant                        |                                                                |
| GET    | `/support/tickets`                    | mine / `tickets:read:all`          | filters `status`, `priority`, `category`, `assignee_id`, `mine=1` |
| POST   | `/support/tickets`                    | `tickets:create`                   | `{category, priority, subject, body}` -> ticket + thread; SLA set |
| GET    | `/support/tickets/{id}`               | requester / agent                  | ticket + thread messages                                       |
| POST   | `/support/tickets/{id}/messages`      | requester / agent                  | `{body}`; first agent reply sets `first_response_at`           |
| POST   | `/support/tickets/{id}/assign`        | `tickets:manage`                   | `{assignee_id}` -> triaged                                     |
| POST   | `/support/tickets/{id}/status`        | agent; requester may close/reopen  | `{status}` with lifecycle rules                                |
| POST   | `/support/tickets/{id}/rate`          | requester                          | `{satisfaction: 1..5}` after resolved                          |

## Admin and platform

| Method | Path                            | Permission     | Notes                                                    |
|--------|---------------------------------|----------------|----------------------------------------------------------|
| GET    | `/admin/users`                  | `users:manage` | with roles, status, last login                            |
| POST   | `/admin/users/{id}/lock`, `/unlock`, `/reset-password` | `users:manage` | reset returns a one-time temporary password |
| PUT    | `/admin/users/{id}/roles`       | `roles:manage` | `{roles:[key]}`                                          |
| GET    | `/admin/roles`                  | `roles:manage` | roles with permissions                                    |
| GET    | `/admin/audit?entity_type=&entity_id=&actor=&from=&to=` | `audit:read` |                                          |
| GET    | `/dashboard`                    | any            | role-aware summary: my tasks, pending approvals, open tickets, shipments in flight, AR outstanding (finance), headcount (HR) |
| GET    | `/healthz`, `/readyz`, `/metrics` | none         |                                                          |

## Internal calls made by the API

- `POST {BILLING_URL}/render/invoice` with `X-Internal-Token`; body = invoice + customer +
  lines; response `{s3_key}`.
- `GET {BILLING_URL}/reports/ar-aging.xlsx?as_of=` proxied for `?format=xlsx`.
- `GET {BILLING_URL}/statements/{customer_id}.pdf?from=&to=` renders a customer
  statement (issued invoices and payments in the window with a running balance).
- `POST {BILLING_URL}/render/document` renders a personnel document. The body carries
  a `kind` of `contract`, `payslip`, `certificate` or `id`, the employee, the object
  key the `employee_documents` row already holds, and the details that kind needs.
  Response `{s3_key, bytes}`; `?inline=1` streams the bytes back instead of storing
  them. Documents go to `S3_BUCKET_DOCUMENTS`, invoices and statements to
  `S3_BUCKET_PDFS`.

- `POST {ANALYTICS_URL}/score/delay-risk` body `{mode, weight_kg, pieces, hazardous,
  distance_km?, carrier_on_time_rate?, etd, eta}` -> `{risk, band, model_version,
  drivers, derived}`; failures are logged and ignored (the shipment is saved without a
  score). `risk` is clamped to [0.001, 0.999] so a calibrated probability never
  asserts certainty.
- `GET {ANALYTICS_URL}/forecast/volume?weeks=&site=` returns a weekly shipment volume
  forecast with an 80% interval, reading history from the database itself. The `POST`
  form of the same path takes `{"series": [{"week_start", "count"}, ...]}` when the
  caller supplies the history instead.

Every internal call carries the shared `X-Internal-Token` header; both services reject
anything else with a 401 problem document.

An invoice PDF is derived from ledger data, so `GET /finance/invoices/{id}/pdf`
re-renders and stores it when the object is missing. A personnel document is a real
file that HR uploaded, so a missing one returns 404 rather than being regenerated:
inventing a replacement would put a document on file that nobody wrote.

