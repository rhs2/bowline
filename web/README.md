# Bowline web

The operator-facing UI for Bowline, a freight operations and workforce platform.
Next.js 15 (App Router), React 19, TypeScript in strict mode, Tailwind CSS.

The browser never talks to the Rust API directly. Every call goes through this app,
which holds the session in httpOnly cookies and forwards requests from the server
side. That pattern (a backend for frontend, or BFF) is the reason the app has route
handlers under `app/api` even though it has no database of its own.

## Running it

Requirements: Node 22 or newer, npm 10 or newer.

```bash
npm install
npm run dev          # http://localhost:3000
```

The dev server starts with no backend running. Pages render their loading state,
then show the RFC 7807 problem the proxy returns (`API unavailable`, HTTP 502) until
the API is up. Nothing crashes and nothing is prerendered against a live API.

### Environment

| Variable                   | Where it is read        | Default                  | Meaning                                                    |
|----------------------------|-------------------------|--------------------------|------------------------------------------------------------|
| `API_INTERNAL_URL`         | server only             | `http://localhost:8080`  | Origin of the Rust API. `/api/v1` is appended by the proxy. |
| `REFRESH_TOKEN_TTL_SECONDS`| server only             | `2592000` (30 days)      | Lifetime of the refresh cookie.                             |
| `NEXT_PUBLIC_APP_NAME`     | build time              | `Bowline`                | Product name in the page title.                             |

`API_INTERNAL_URL` is deliberately not a `NEXT_PUBLIC_` variable. The API origin is
never sent to the browser, so the API can sit on a private network with no public
route at all.

### Scripts

```bash
npm run dev          # development server
npm run build        # production build (Next.js standalone output)
npm start            # run the production build
npm run lint         # eslint, including the next/typescript rules
npm run typecheck    # tsc --noEmit
npm test             # vitest in watch mode; add -- --run for a single pass
npm run format       # prettier
```

### Docker

```bash
docker build -t bowline/web:dev .
docker run --rm -p 3000:3000 -e API_INTERNAL_URL=http://api:8080 bowline/web:dev
```

The image is a three-stage build on `node:22-alpine`. The runtime stage carries only
the Next.js standalone server, its traced dependencies and the static assets, runs as
the unprivileged `node` user, exposes port 3000, and has a `HEALTHCHECK` that fetches
`/login` (the one page that renders without a session).

## The backend for frontend

### Why

Access tokens are short lived and refresh tokens rotate on every use, with reuse
revoking the whole token family. Neither belongs in `localStorage` where any script
on the page can read them. Instead both live in httpOnly cookies that JavaScript
cannot touch, and a thin server-side proxy attaches the bearer token on the way out.

### The pieces

```
browser                    this app (Node runtime)                 Rust API
-------                    -----------------------                 --------
fetch("/api/proxy/...")
  cookies attached
  automatically      ->    app/api/proxy/[...path]/route.ts   ->   /api/v1/...
                           reads bowline_access from cookie        Authorization: Bearer
                           on 401, refreshes once and retries
                     <-    response streamed back unchanged   <-
```

- `app/api/auth/login/route.ts` exchanges credentials for a token pair and writes the
  cookies. The browser only ever receives `{must_change_password, expires_in}`.
- `app/api/auth/refresh/route.ts` rotates the pair and rewrites both cookies.
- `app/api/auth/logout/route.ts` revokes the refresh token upstream and clears the cookies.
- `app/api/auth/me/route.ts` returns the principal, refreshing first if the access
  token has expired. This is the one call that can rotate the session for a server
  component, which cannot set cookies itself.
- `app/api/proxy/[...path]/route.ts` is the catch-all. `/api/proxy/<path>?q` becomes
  `${API_INTERNAL_URL}/api/v1/<path>?q`.

### Cookies

| Cookie             | Contents                    | Notes                                              |
|--------------------|-----------------------------|----------------------------------------------------|
| `bowline_access`   | access token                | httpOnly, sameSite lax, secure in production        |
| `bowline_refresh`  | refresh token               | same flags, longer max age                          |
| `bowline_pwchange` | `1` when a change is forced | not a secret; lets the edge middleware redirect     |

`middleware.ts` runs at the edge on page routes only. It redirects visitors with no
session to `/login` (keeping the requested path in `?next=`) and pins accounts that
must change their password to `/change-password`. It never sees a token's contents,
only whether a cookie is present, so it stays fast and cannot leak anything.

### Transparent refresh, in two layers

1. **Server side.** When the proxy gets a 401 from the API and a refresh cookie is
   present, it refreshes once, retries the request, and writes the rotated cookies
   onto the same response. Concurrent requests carrying the same refresh token share
   a single upstream refresh call (`lib/server/upstream.ts` keeps an in-flight map),
   because a replayed refresh token would revoke the family.
2. **Client side.** If a 401 still comes back (for example the Node process restarted
   and lost the in-flight refresh), `lib/api.ts` calls `/api/auth/refresh` once and
   retries the request, again sharing one in-flight refresh between callers. If that
   fails, the session is cleared and the browser goes to `/login?next=...`.

The result: a user working through a long shift never sees a session expire, and no
page ever has to think about tokens.

### Calling the API from a page

Always through `lib/api.ts`. Never `fetch` an API origin directly.

```ts
const list = useList<Shipment>("ops/shipments", { status, mode });     // paginated
const detail = useQuery<ShipmentDetail>(`ops/shipments/${id}`);        // one resource
const act = useAction(() => api.post(`ops/shipments/${id}/transition`, { to }));
```

`useQuery` and `useList` (in `lib/hooks.ts`) handle aborting, reloading and the list
envelope. `useAction` (in `lib/forms.ts`) handles pending state, toasts, and pulls
`errors[]` out of a 422 problem so forms can show messages against the right field.
`proxyUrl()` builds a URL for links the browser opens directly, such as the AR aging
spreadsheet.

### Rendering and the build

The build must succeed with no backend running, so nothing fetches during
`next build`. The app shell layout is `export const dynamic = "force-dynamic"`, and
every data page is a client component that fetches after hydration. The build output
marks all of them `ƒ (Dynamic)`. Only `/login`, `/change-password` and `/_not-found`
are static, and none of them touch the API at build time.

## Page map by role

Sidebar entries are filtered by permission in `lib/nav.ts`, so a link only appears
when the page behind it can actually load. Permission checks use `lib/permissions.ts`,
which understands the scope ladder (`all` beats `department` beats `subtree` beats
`self`) as well as the families with their own suffixes.

### Everyone (the baseline role set)

| Page                | Route             | What it does                                                          |
|---------------------|-------------------|-----------------------------------------------------------------------|
| Dashboard           | `/dashboard`      | Role-aware counters and quick actions                                  |
| Inbox               | `/inbox`          | Direct message threads                                                 |
| Announcements       | `/announcements`  | Company, department and subtree broadcasts                             |
| Support             | `/support`        | Raise and follow Service Desk tickets, with the SLA countdown          |
| Org chart           | `/org`            | The whole tree, names and titles only                                  |
| Leave               | `/hr/leave`       | Balances, requests, and the approvals tab for approvers                |
| Shifts              | `/hr/shifts`      | Upcoming and past roster                                               |
| Attendance          | `/hr/attendance`  | Clock in and out, with a late indicator on the history                 |
| Documents           | `/hr/documents`   | Your contracts, certificates and payslips                              |
| Work orders         | `/ops/work-orders`| Your task list, built for a phone                                      |
| Shipments           | `/ops/shipments`  | Read-only board for anyone holding `shipments:read`                    |
| Expenses            | `/finance/expenses`| Submit a claim and follow its approval steps                          |

### Field worker (drivers, dock crews, handlers)

Baseline only. In practice the pages that matter are `/ops/work-orders`,
`/hr/attendance`, `/hr/shifts`, `/inbox` and `/support`. Work orders use large touch
targets and one-tap start, done and blocked actions.

### Staff and coordinators (`shipments:write`, `customers:read`)

Adds shipment creation, the transition buttons on `/ops/shipments/[id]`, the add-event
form, document upload against a shipment, and the customer list.

### Supervisor and manager

Adds the approvals tab on `/hr/leave`, the team tab on `/ops/work-orders`, scheduling
on `/hr/shifts` (`shifts:manage:subtree`), the expense approval queue, and `/people`
for their subtree. Managers see their subtree's document list but not the files.

### Dispatcher (`shipments:assign`, `fleet:manage`)

Adds the add-leg form on a shipment and full create and edit on `/ops/fleet` across
the carriers, sites and vehicles tabs.

### Accountant (`ledger:read`, `ledger:post`, `invoices:draft`, `invoices:issue`, `payments:record`)

| Page              | Route                       | What it does                                                        |
|-------------------|-----------------------------|---------------------------------------------------------------------|
| Invoices          | `/finance/invoices`         | Filter by status, customer and overdue, with per-currency totals     |
| Invoice           | `/finance/invoices/[id]`    | Lines, totals, payments, status actions, and the rendered PDF        |
| Ledger            | `/finance/ledger`           | Journal entries with their lines, and the manual entry form          |
| Payroll           | `/finance/payroll`          | Runs and their items; create needs `payroll:prepare`                 |
| Reports           | `/finance/reports`          | Trial balance, AR aging with bucket totals and xlsx, profit and loss  |

The manual entry form keeps running debit and credit totals and refuses to submit
while the entry is out of balance, mirroring the deferred constraint in the database.

### Finance admin (adds `invoices:approve`, `expenses:approve:finance`, `payroll:approve`)

The approve and void buttons appear on an invoice, the payroll approve and post
actions unlock, and the expense queue shows claims at the finance step. The invoice
and expense action sets come from `lib/transitions.ts`, so a button is only rendered
when the current status and the caller's permissions both allow it.

### HR admin (`employees:write:all`, `leave:manage:all`, `documents:manage:all`, `users:manage`)

Adds employee creation and termination on `/people`, leave decisions on anyone's
behalf, the employee picker and upload panel on `/hr/documents`, and `/admin/users`.

### IT admin and auditor

| Page      | Route           | Permission     | What it does                                                        |
|-----------|-----------------|----------------|---------------------------------------------------------------------|
| Users     | `/admin/users`  | `users:manage` | Roles, status, last login, lock and unlock, one-time password reset  |
| Roles     | `/admin/roles`  | `roles:manage` | Every role with its permission keys grouped by family                |
| Audit log | `/admin/audit`  | `audit:read`   | Filter by entity type, entity id, actor and date range               |

A password reset shows the temporary password exactly once, in a modal, with a copy
button and a plain warning that it cannot be retrieved again.

## Testing

`vitest` with `jsdom` and Testing Library. The suite deliberately concentrates on the
pure logic that decides what a user is allowed to see and do, because that is where a
bug is silent and expensive. Rendering is covered where a component encodes a rule.

```bash
npm test -- --run
```

| File                              | Covers                                                                    |
|-----------------------------------|---------------------------------------------------------------------------|
| `lib/transitions.test.ts`         | The shipment state machine, the expense approval steps, and the invoice, work order, payroll and ticket action sets |
| `lib/ledger.test.ts`              | Integer-cent money arithmetic and the journal entry balance validator      |
| `lib/nav.test.ts`                 | Sidebar filtering for a field worker, an accountant, an IT admin and a dispatcher |
| `components/DelayRisk.test.tsx`   | Delay risk banding and its rendering, including the unscored case          |

What the tests are checking, in words:

- A shipment can never skip a step in the happy path, can only be cancelled from
  `draft` or `booked`, offers nothing once it is `delivered` or `cancelled`, and
  resumes an `exception` only into the state it came from.
- Money never goes through a binary float. `"0.10"` plus `"0.20"` is exactly
  `"0.30"`, and an entry is balanced only when both sides match and are non-zero.
- An expense claim waits on the manager first and then on finance, and a manager
  holding only `expenses:approve:subtree` cannot act on a claim that has moved past
  their step.
- The sidebar shows an accountant the finance section and hides administration, and
  shows a field worker neither.

Everything else is exercised by `npm run typecheck` (strict mode with
`noUncheckedIndexedAccess`) and by `npm run lint`, which bans `any` and stray
`console.log` on every path.

## Layout of the source

```
app/
  (app)/            pages behind the app shell, one folder per section
  api/auth/*        login, logout, refresh, me
  api/proxy/        the catch-all BFF proxy
  login/            the only page that renders without a session
components/
  ui/               buttons, fields, tables, modals, badges, toasts
  pickers/          debounced search pickers for people, customers, recipients
  shell/            sidebar, top bar, app shell
lib/
  api.ts            the typed client, every call goes through it
  hooks.ts          useQuery, useList, useDebounced, useNow
  forms.ts          useAction, problem capture, field errors
  permissions.ts    scope-aware permission checks
  nav.ts            the permission-filtered sidebar
  transitions.ts    state machines and permission-aware action sets
  ledger.ts         integer-cent money arithmetic
  types.ts          the API contract as TypeScript
  server/           server-only helpers, never imported by a client component
```
