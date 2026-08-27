# Bowline security

What Bowline defends, how it defends it, and what it deliberately does not
defend. `ARCHITECTURE.md` describes the system, `DOMAIN.md` describes the roles
and rules, `RUNBOOK.md` describes what to do when something goes wrong. This file
is the security posture, and the last section is the honest list of what is
missing.

Bowline is a portfolio implementation of a production system. Everything
described here is in the repository and can be read. Where a control is designed
but not implemented, this document says so rather than implying otherwise.

## 1. Threat model

### What is worth protecting

| Asset | Why it matters |
|---|---|
| Employee records | Names, addresses, salaries, contracts, payslips, disciplinary context. The most sensitive data in the system and the least useful to an attacker commercially, which makes it an insider risk more than an external one. |
| The ledger | Double-entry finance for the whole company. Integrity matters more than confidentiality: a silently altered journal line is worse than a leaked one. |
| Invoices and customer terms | Credit limits, pricing and volumes. Commercially sensitive to competitors. |
| Credentials and tokens | Password hashes, refresh tokens, the JWT signing key, the internal service token, database passwords, SES SMTP credentials. |
| The reporting hierarchy | Not secret in itself, but it is the input to every authorisation decision, so tampering with it is privilege escalation. |
| The audit log | The record of who did what. Its value is entirely in being unalterable. |

### Who we are defending against

**The unauthenticated internet.** Automated credential stuffing, scanning for
exposed endpoints, and attempts to reach the database or internal services
directly. This is the loudest and least targeted threat.

**An authenticated employee acting beyond their remit.** The main threat this
system is designed around. Bowline has roughly 260 users spanning drivers to the
CEO, all of whom hold a valid token. A driver reading the CFO's salary, a
supervisor approving their own expense claim, or a coordinator listing every
employee in the company are all failures even though every request was
authenticated. This is why authorisation, not authentication, is where most of
the engineering went.

**A compromised employee account.** Phished or reused password. The controls that
matter are short access token lifetime, refresh token reuse detection, lockout,
and the fact that even a fully compromised account is confined to that user's
scope.

**A compromised service.** If the `billing` container is exploited, the blast
radius should be a read-only database role and one S3 bucket, not the whole
system. This drives the per-service task roles and the three database roles.

**An operator making a mistake.** Not malicious, but the most likely cause of an
incident: applying the wrong environment, running a migration against the wrong
database, deleting the wrong instance. Guard rails such as deletion protection,
the protected GitHub environment and the environment name validation in each
Terraform root exist for this actor.

### Explicitly not in the threat model

A malicious AWS account administrator, a compromise of AWS itself, a supply chain
attack on a pinned dependency that Dependabot has not yet flagged, and physical
access to a user's unlocked machine. Each of these defeats the design.

### Trust boundaries

```
  internet
     |  TLS 1.2/1.3, ALB, HTTP 301 to HTTPS               <-- boundary 1
     v
  ALB (public subnets)
     |  security group to security group, ports 8080/3000  <-- boundary 2
     v
  ECS tasks (private subnets, no public IP, egress via NAT)
     |  X-Internal-Token to billing and analytics          <-- boundary 3
     |  TLS to PostgreSQL and Redis
     v
  RDS and ElastiCache (isolated subnets, no internet route at all)
                                                           <-- boundary 4
```

Boundary 1 is the only one reachable from outside AWS. Boundary 4 has no route to
the internet in either direction: the isolated subnets share a route table
carrying nothing but the VPC-local route.

## 2. Authentication

Implemented in `api/src/auth`.

### Passwords

Hashed with **Argon2id**, `m = 65536` KiB (64 MiB), `t = 3`, `p = 1`, the
parameters from `api/src/auth/password.rs`. Salts are per-password from the OS
random source; the full PHC string including the parameters is stored, so the
cost can be raised later and old hashes still verify.

Passwords never appear in logs, in the audit log, or in an API response. The
`before` and `after` snapshots the audit log records for a user change exclude
the hash.

**Login does a dummy verify on an unknown email.** `api/src/auth/handlers.rs`
carries a constant `DUMMY_HASH` and verifies against it when no user matches, so
a request for an address that does not exist costs the same 64 MiB of hashing as
one that does. Without this, response timing tells an attacker which company
addresses are real, which for a company with a predictable email format is a
complete staff directory.

### Access tokens

HS256 JWTs, **valid 15 minutes**, carrying only the user id and a token version
(`tv`) claim. No roles, no permissions and no employee data are in the token. The
permission set is loaded from the database on every request and cached in Redis
for 60 seconds, so a role change takes effect within a minute rather than at the
next login. A token is a claim about identity, never about authority.

The signing key comes from `JWT_SECRET`, at least 32 random bytes, held in
Secrets Manager. Rotation is in `RUNBOOK.md` section 5.

### Refresh tokens, rotation and family reuse detection

Refresh tokens are **not JWTs**. Each is 32 random bytes, delivered to the browser
in an httpOnly cookie and stored in `refresh_tokens` as a SHA-256 hash, so a
database read does not yield usable tokens. They are valid 30 days.

Every refresh **rotates**: the presented token is marked revoked, its
`replaced_by` points at the new one, and a new token is issued. Each login starts
a **family**, identified by `family_id`, and every rotation stays in that family.

This gives detection of token theft. If a refresh token is presented twice, one of
two things happened: the legitimate client is retrying, or an attacker is
replaying a token that the legitimate client already rotated. The system cannot
tell which, so it assumes the worse case and **revokes the entire family**. Both
the attacker and the real user are logged out, and the real user logs in again,
which starts a fresh family. The alternative, accepting the replay, means a stolen
token works indefinitely because the thief simply keeps rotating it.

The schema supports this directly:

```sql
create table refresh_tokens (
  id          uuid primary key default gen_random_uuid(),
  user_id     uuid not null references users(id) on delete cascade,
  family_id   uuid not null,        -- one family per login; reuse revokes the family
  token_hash  text not null unique, -- sha256 of the opaque token
  expires_at  timestamptz not null,
  revoked_at  timestamptz,
  replaced_by uuid references refresh_tokens(id) on delete set null,
  user_agent  text,
  ip          inet,
  created_at  timestamptz not null default now()
);
```

### Lockout

Five consecutive failed logins lock the account for 15 minutes
(`LOGIN_MAX_FAILURES`, `LOGIN_LOCKOUT_SECONDS`). The counter is `failed_logins`
on `users` and the deadline is `locked_until`; a successful login resets both.
This is the control against online password guessing, and it is per account
rather than per IP so that a distributed attempt is still caught.

It is also a denial of service against a named user, which is the accepted
trade: an attacker who knows an address can lock that person out for fifteen
minutes at a time. The alternative, no lockout, makes the password the only
barrier.

Separately, `RATE_LIMIT_PER_MINUTE` (300) is enforced per user for authenticated
routes and per IP for anonymous ones, backed by Redis.

### Forced password change

`users.must_change_password` defaults to **true**. Every account created by an HR
admin or by the seed starts with a temporary password and cannot do anything
except change it. The API rejects other calls until it is cleared, so an
administrator who sets a password and reads it out over the phone has not created
a permanent shared credential.

Changing a password **increments `token_version`**, which invalidates every
existing access token for that user and every cached principal. A password change
after a suspected compromise therefore ends the attacker's session rather than
leaving it running for up to fifteen minutes. Terminating an employee does the
same thing and additionally deletes their refresh tokens.

## 3. Authorisation

Authentication answers "who is this". Everything below answers "what may they see
and do", and it is evaluated on every single request. There are no unscoped list
endpoints anywhere in the API.

### The reporting path

`employees.path` is a PostgreSQL `ltree` holding the chain of employee ids from
the CEO down to that employee. It is maintained by triggers: inserting an employee
computes their path from their manager, re-parenting rewrites the whole subtree in
one statement, and a change that would create a cycle is rejected.

This is the single source of truth for every hierarchical question. "Everyone
below me" is `path <@ :my_path`, a GiST index lookup rather than a recursive
query, which is what makes it cheap enough to put in the `where` clause of every
list endpoint rather than filtering after the fact.

The security property that matters is that there is **no second copy of the org
chart**. A permission that says "my team" and a report that says "my team"
consult the same column, so they cannot drift apart. Re-parenting is a privileged
operation precisely because it silently changes what a set of people can see.

### The permission catalogue

Permissions are keys of the form `resource:action[:scope]`, stored in the
`permissions` table and seeded by `db/migrations/0008_reference_data.sql`. Roles
are named bundles of keys in `role_permissions`; a user holds one or more roles
through `user_roles` and the effective permission set is their union, exposed as
the `user_permissions` view.

Fifteen roles are seeded, from `field_worker` up through `supervisor`, `manager`,
`director` and `executive`, alongside the functional roles `hr_admin`,
`accountant`, `finance_admin`, `dispatcher`, `support_agent`, `it_admin` and
`auditor`. `DOMAIN.md` holds the full table. Two properties matter here:

**Roles are additive and composable.** A Billing Manager is `manager` plus
`finance_admin`, not a bespoke role. This keeps the catalogue small enough to
review, and it means a permission question can be answered by reading two rows
rather than by reasoning about role inheritance.

**The catalogue is data, not code.** Adding a permission is a migration. There is
no code path that grants an implicit capability, and nothing is keyed off a role
*name*: handlers check permission keys. An `executive` has broad access because
the seed gives that role broad keys, not because the code has a special case for
executives. There is no superuser bypass, including for `it_admin`.

### The four scopes

Most permission families end in a scope suffix. Given a family such as
`employees:read`, the effective scope is the **widest one the principal holds**,
ordered `all` > `department` > `subtree` > `self`. Every query then adds the
corresponding predicate:

| Scope | Predicate | Meaning |
|---|---|---|
| `self` | `employee_id = :me` | Only my own record |
| `subtree` | `path <@ :my_path` | Me and everyone below me |
| `department` | `department_id in (mine and its children)` | My department branch |
| `all` | no filter | Everything |

The scope is resolved once, in the domain service, and applied in the SQL. It is
not applied by filtering results in the handler, because a filter after the fact
still lets counts, pagination totals and error messages leak the existence of
rows the caller may not see.

Resources that are not employee-shaped, such as shipments, invoices and support
tickets, use their own permission keys rather than being forced into the
hierarchy.

### Why detail endpoints return 404 and not 403

A request for an employee outside the caller's scope returns **404 Not Found**,
not 403 Forbidden. This is deliberate.

403 means "this exists and you may not see it". That single bit is itself
information, and in a company hierarchy it is worth a lot. A driver who can
distinguish 403 from 404 across a range of employee ids can enumerate exactly
which ids are real people, and by probing paths can reconstruct the shape of the
organisation, including the existence of a department or an executive they were
never meant to know about. In a system where the org chart *is* the access
control model, letting people map it is letting them map the security boundary.

So the API treats "outside your scope" and "does not exist" as the same answer.
In `api/src/org/handlers.rs` the scoped query returns no row and the handler
raises `ApiError::not_found("employee")` without ever distinguishing the two
cases. Errors are RFC 7807 problem documents with a stable `code`, and the
`not_found` code is identical in both situations.

403 is still used where the resource is not in question and only the action is:
attempting to approve a leave request you can see but may not approve, or issuing
an invoice over the approval threshold without `invoices:approve`. The rule is
that **404 hides existence, 403 refuses an action on something already visible**.

The cost is worse error messages. A manager who mistypes an id and one who lacks
permission get the same response, and support cannot tell them apart from the
client side. The audit log and the API logs record which it was, so the
information exists for an operator; it is just not given to the caller.

### Service to service

`billing` and `analytics` are called by the API over HTTP inside the VPC and
authenticate with a shared secret in the `X-Internal-Token` header. Both compare
in constant time: `hmac.compare_digest` in `analytics/analytics/auth.py`, and
`MessageDigest.isEqual` in `InternalTokenFilter.java`. Both reject before any
handler runs, both answer `problem+json` 401, and both refuse to start at all if
the token is unset rather than defaulting to open. Only `/healthz` and `/metrics`
are exempt.

This is a bearer secret shared by three services, not per-request authorisation.
It stops anything that reaches the port without the token; it does not stop a
compromised `api` from asking `billing` for a document it should not have. The
network boundary and the read-only database role are what limit that.

## 4. Data protection

### At rest

| Store | Encryption |
|---|---|
| RDS PostgreSQL | `storage_encrypted = true`, KMS. Automated backups and snapshots inherit it. |
| ElastiCache Redis | `at_rest_encryption_enabled = true` |
| S3 documents and PDFs | SSE-KMS with a customer managed key created by the `storage` module, `enable_key_rotation = true`, `bucket_key_enabled = true` |
| Secrets Manager | KMS, the AWS managed key by default, a customer managed key if `secrets_kms_key_id` is set |
| ECR images | AES256 |
| Terraform state | The bootstrap bucket is created with SSE and versioning (`infra/README.md`) |

Both buckets additionally have `BucketOwnerEnforced` ownership (ACLs off), a full
public access block, versioning on, and a lifecycle rule expiring noncurrent
versions after 180 days in production.

### In transit

- **Browser to ALB**: HTTPS only. The `:80` listener does nothing but a 301 to
  `:443`. The HTTPS listener uses `ELBSecurityPolicy-TLS13-1-2-2021-06`, so TLS
  1.2 is the floor and 1.3 is preferred. `drop_invalid_header_fields = true` on
  the load balancer, which removes malformed headers before they reach a target
  and closes off a class of request smuggling.
- **ALB to tasks**: HTTP inside the VPC, between two security groups, in private
  subnets with no public IP. Not encrypted, which is the standard trade for
  Fargate behind an ALB.
- **Tasks to PostgreSQL**: TLS enforced on **both** ends. The parameter group sets
  `rds.force_ssl = 1`, so the server refuses a plaintext connection, and every
  generated connection URL carries `sslmode=require`. `ca_cert_identifier` is
  pinned.
- **Tasks to Redis**: `transit_encryption_enabled = true` with an AUTH token, and
  the URL scheme is `rediss://`.
- **Tasks to S3**: HTTPS through the SDK, and each bucket policy carries an
  explicit `Deny` on `s3:*` when `aws:SecureTransport` is false. That covers
  presigned URLs too, which are handed to browsers and therefore leave the VPC.
- **Outbound mail**: the SES configuration set sets `tls_policy = REQUIRE`, so
  SES will not fall back to plaintext delivery, and the worker connects on 587
  with STARTTLS.

### Response headers and browser-side controls

The API sets, on every response: `X-Content-Type-Options: nosniff`,
`X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`, and
`Strict-Transport-Security: max-age=31536000; includeSubDomains`. API and docs
responses additionally carry `Cache-Control: no-store` and
`Content-Security-Policy: default-src 'none'; frame-ancestors 'none'`.

CORS is an allow-list, not a wildcard: origins come from `API_CORS_ORIGINS`, and
in production that is the single application origin. Because the web app and the
API share one hostname behind the ALB (path rules send `/api/*`, `/docs` and
`/api-docs/*` to the API and everything else to the web app), the browser never
makes a cross-origin call in normal operation. Allowed headers are limited to
`Authorization`, `Content-Type` and the request id header.

The refresh token is an httpOnly cookie so JavaScript cannot read it; the access
token is held in memory by the web app rather than in `localStorage`.

### Documents

Uploaded bytes never pass through the API process. The API issues a presigned S3
PUT URL, the browser uploads directly, and a confirmation call records the key,
size and MIME type. Downloads are presigned GETs. Presigned URLs expire after
`PRESIGN_TTL_SECONDS` (900). The authorisation check happens when the URL is
issued, which means a URL that has been issued stays valid for its lifetime even
if permission is revoked in the meantime. Fifteen minutes is the chosen exposure.

### Data in logs

Structured API logs carry `request_id`, `user_id`, route, status and latency.
They do not carry request bodies, so names, addresses and salaries do not reach
CloudWatch. The audit log holds before and after snapshots and *does* contain
business data, but it lives in the database under the same authorisation rules as
everything else, not in a log aggregator.

## 5. Secrets handling

**No secret is in the repository.** `.env.example` documents every variable the
system reads and carries development placeholders that are obviously not real.
The root ignore file excludes `.env`, and `infra/.gitignore` excludes
`terraform.tfvars` and saved plans.

**Everything is generated, not chosen.** The Terraform `secrets`, `database`,
`cache` and `mail` modules generate the JWT signing key, the internal service
token, four database passwords and the SES SMTP credential. No human picks or
sees these values in the course of a normal deploy. Per environment:

| Secret | Contents | Read by |
|---|---|---|
| `bowline/<env>/jwt-secret` | 32 random bytes as hex | `api` |
| `bowline/<env>/internal-service-token` | 48 character token | `api`, `billing`, `analytics` |
| `bowline/<env>/db/master` | RDS master credential | the `bowline-migrate-<env>` task only |
| `bowline/<env>/db/app` | `bowline_app` | `api` |
| `bowline/<env>/db/ro` | `bowline_ro` | `billing`, `analytics` |
| `bowline/<env>/db/notify` | `bowline_notify` | `notify` |
| `bowline/<env>/redis` | AUTH token and `rediss://` URL | `api` |
| `bowline/<env>/smtp` | SES SMTP username and password | `notify` |
| `bowline/<env>/app` | JSON bundle of the application secrets | nothing by default |

**Injection is per container.** Task definitions reference secrets by ARN in the
`secrets` block, and the ECS agent resolves them at task start using the
**execution role**. The task role, which is what application code can use, has no
Secrets Manager permission at all. The execution role's policy names exactly the
ARNs above and nothing else, with `kms:Decrypt` added only when a customer
managed key is in use.

Consequently each container receives only its own credentials. The `notify`
container cannot read the JWT key; the `web` container has no secrets at all.

**The master database credential is the tightly held one.** It exists so the
migrate task can create the three application roles on first run. No long-running
service has it, and it is the only credential that can alter the schema.

Because it is the most dangerous credential in the system, the task definition
that references it is named per environment: `bowline-migrate-<env>`, never a
shared `bowline-migrate`. ECS task definition families are account-wide, and
`run-task` given a family with no revision number resolves to the newest revision
of that family. A single shared family would receive revisions from both
environments, and each revision embeds the secret ARNs of the environment that
created it. Whichever environment was applied most recently would win, so a
staging deploy could run migrations against the production database, holding the
production master credential, from a job nobody was watching closely because it
was only staging. Putting the environment in the family name makes the two
unable to resolve to each other. The `ecs` module derives the name from its
`environment` variable and `deploy.yml` resolves the same string, so they cannot
drift apart. The workflow also waits for the task and fails the deploy on a
non-zero exit code, so a migration that fails is visible rather than silent.

**Rotation is a Terraform operation**, `apply -replace` on the generating
resource followed by a service restart, documented step by step in `RUNBOOK.md`
sections 5 and 6, including which order to restart services in so the internal
token rotation does not open a gap.

**Deploy credentials are short lived.** GitHub Actions holds no AWS access key.
`deploy.yml` requests an OIDC token and exchanges it for a role session capped at
one hour, and the role's trust policy pins the audience, the repository, the
branch and the GitHub environment (`infra/README.md`). The only AWS value in
GitHub is a role ARN, which is not a secret.

## 6. Network boundaries

The `network` module builds three subnet tiers per availability zone:

| Tier | Contains | Route to internet |
|---|---|---|
| public | The ALB only | Internet gateway |
| private | ECS tasks | Outbound through NAT, no inbound |
| isolated | RDS, ElastiCache | **None in either direction** |

Tasks run with `assign_public_ip = false` and are never addressable from outside.
The isolated tier's route table carries only the VPC-local route and the S3
gateway endpoint, so there is no path by which the database could reach or be
reached by the internet even if its security group were opened.

**Security groups reference each other, not CIDR blocks.** The database accepts
5432 from the ECS security group, not from a subnet range, so a future resource
placed in the same subnet does not silently inherit database access. Rules are
individual `aws_vpc_security_group_*_rule` resources so each carries its own
description and appears by name in a plan.

Egress is restricted, not just ingress:

- The **ALB** may egress only to the ECS group on 8080 and 3000. A compromised
  load balancer cannot be used to reach anything else.
- **Tasks** may reach the database (5432), the cache (6379), each other on the
  internal service ports, the VPC endpoints, `0.0.0.0/0:443` and
  `0.0.0.0/0:587`. There is no general outbound rule.
- The **database and cache groups have no egress rules at all**. Nothing they
  could be made to run can call out.

**Production adds interface VPC endpoints** for ECR API, ECR Docker, CloudWatch
Logs and Secrets Manager, so image pulls, log shipping and secret retrieval never
traverse NAT or the public internet.

**`/metrics` is not reachable from outside.** Every service exposes a Prometheus
endpoint, and the HTTPS listener carries an explicit rule at priority 5 returning
a fixed 404 for `/metrics` before any forwarding rule is evaluated. The endpoints
are reachable only from inside the security group.

**VPC flow logs** record accepted and rejected connections to CloudWatch, 90 days
in production and 7 in staging.

**ECS Exec is off in production** (`enable_execute_command = false`). Turning it
on is a Terraform change that rolls the services, so it is visible in a plan and
in the deploy history, and every session is recorded in CloudTrail.

**IAM is per service, not per cluster.** Each of the five services plus the
migrate task gets its own task role. Only `api` and `billing` get S3 access, and
only to the buckets they use with the verbs they need: `api` may put, get and
delete in both buckets, `billing` may put and get in the PDF bucket and cannot
delete anything. Only `notify` may call SES. The `ecs-tasks.amazonaws.com` trust
policy is additionally conditioned on `aws:SourceAccount` and `aws:SourceArn` to
prevent cross-account confused deputy use.

## 7. Least-privilege database roles

One database, three application roles, created by the migrate task and granted in
`db/migrations/0007_audit_outbox.sql`. No service connects as the owner or as the
master user.

| Role | Grants | Used by |
|---|---|---|
| `bowline_app` | Read and write across the schema. Owns the objects. | `api` |
| `bowline_ro` | `usage` on the schema and `select` on all tables. Nothing else. | `billing`, `analytics` |
| `bowline_notify` | `usage` on the schema, and `select, update` on `notifications` **only**. | `notify` |

This is the containment story for a compromised service. If `analytics`, which
parses model input and is the most exposed to malformed data, is exploited, the
attacker reaches a role that cannot write a single row anywhere. If `notify` is
compromised, the attacker can mark notifications sent and read them, and cannot
touch employees, invoices or the ledger. Neither can create a user or grant
themselves anything.

`bowline_notify` has no `delete`, so the worker cannot destroy the outbox record
of a message it failed to deliver. The `api` role is the only writer of business
data, which is what makes the audit log complete: there is no second writer to
bypass it.

The grants are applied conditionally (`if exists (select 1 from pg_roles ...)`)
so the migrations still run on a bare test database where the roles were never
created.

Integrity is also enforced at the database rather than only in the application,
which means a hypothetical second writer still could not corrupt it: a deferred
constraint trigger asserts debits equal credits per journal entry at commit,
journal lines are immutable, and posting into a closed fiscal period is rejected
by trigger.

## 8. The audit log

Every mutation writes a row into `audit_log` **in the same transaction as the
change**. If the business write rolls back, so does its audit row; if the audit
write fails, the business change fails with it. There is no path that commits a
change without its record.

```sql
create table audit_log (
  id                bigserial primary key,
  at                timestamptz not null default now(),
  actor_user_id     uuid,
  actor_employee_id uuid,
  action            text not null,   -- employee.update, invoice.issue, ...
  entity_type       text not null,
  entity_id         uuid,
  before            jsonb,
  after             jsonb,
  ip                inet,
  request_id        text
);
```

`request_id` ties a row to the API log line and to any downstream call in the
same request, so an audit entry can be traced back to the exact HTTP request that
caused it.

### The append-only guarantee

It is enforced by the database, not by convention:

```sql
create or replace function audit_log_immutable() returns trigger
language plpgsql as $$
begin
  raise exception 'audit_log is append-only' using errcode = 'check_violation';
end $$;

create trigger audit_log_no_change before update or delete on audit_log
  for each row execute function audit_log_immutable();
```

Any `update` or `delete`, from any role, through any code path, raises. The API
cannot rewrite history, and neither can an operator with a `psql` session as
`bowline_app`. This is why `RUNBOOK.md` tells an operator not to trim the audit
log when RDS storage is low, and why doing so has to be a reviewed schema change
with an archive table rather than a one-line fix during an incident.

The guarantee is against *application-level* tampering. It is not tamper
evidence: there are no hash chains and no write-once storage, so someone with the
RDS master credential could drop the trigger. Detecting that is what CloudTrail
and the fact that the master credential lives only in the migrate task are for.

Reading the log requires `audit:read`, held by `executive`, `it_admin` and
`auditor`. The `auditor` role is read-only everywhere by design, so an external
audit can be given real access without any ability to change anything.

## 9. Dependency and container scanning

**Dependency updates** are handled by Dependabot, configured in
`.github/dependabot.yml` across all eight ecosystems in the repository weekly:
Cargo (`/api`), npm (`/web`), Maven (`/billing`), pip (`/analytics`), Go modules
(`/tools`), Terraform (`/infra/terraform`), Docker, and GitHub Actions. Updating
the workflow actions themselves matters as much as the application dependencies,
since those actions hold the OIDC token.

**Versions are pinned everywhere.** `analytics/requirements.txt` pins every
package exactly, the Terraform AWS provider is `~> 5.70` with
`required_version >= 1.9`, the CI Terraform version is `1.9.8`, and base images
in `docker-compose.yml` are pinned to specific releases rather than floating
tags. A pinned dependency is what makes a Dependabot pull request a reviewable
change rather than a surprise.

**Vulnerability auditing is blocking in CI.** The `security` job in `ci.yml` runs
four auditors, one per language, and any finding fails the build:

| Auditor | Scope | Threshold |
|---|---|---|
| `cargo audit --deny warnings` | `api` | Warnings are failures, so an unmaintained crate fails too, not only an advisory |
| `npm audit --audit-level=high` | `web` | High and critical |
| `pip-audit` | `analytics`, both `requirements.txt` and `requirements-dev.txt` | Any advisory |
| `govulncheck ./...` | `tools` | Reachable vulnerabilities only, so it does not fail on a vulnerable symbol the code never calls |

**Container images are scanned twice, at different points.** In CI, the `docker`
job builds all six images (`api`, `web`, `billing`, `analytics`, `notify`,
`bowctl`), loads each into the runner and scans it with Trivy at `HIGH,CRITICAL`
with `ignore-unfixed: true` and `exit-code: 1`, so a fixable high or critical
vulnerability in a base image or an OS package fails the build before the image
can ever be pushed. In AWS, every ECR repository is created with
`scan_on_push = true`, so images are re-scanned against the CVE database on push
from `deploy.yml` and continue to be assessed after the build, which is what
catches an advisory published after a build went green. The lifecycle policy keeps
the last twenty images and expires untagged layers after seven days, bounding how
much unscanned history accumulates.

`ignore-unfixed: true` is a deliberate choice: it excludes vulnerabilities with no
available patch, so the gate stays actionable rather than becoming noise a team
learns to bypass. Those findings still surface in ECR, where they can be reviewed
without blocking a deploy.

**Static analysis in CI** is per language and also blocking: `cargo clippy
--all-targets -- -D warnings` (warnings are errors), `cargo fmt --check`, `ruff
check` and `ruff format --check`, `go vet`, `gofmt`, and the web lint and
typecheck. `terraform fmt -check -recursive` and `terraform validate` cover every
module and environment directory.

**The layering is intentional.** Dependabot proposes upgrades, the audit job stops
a known-vulnerable dependency from merging, Trivy stops a vulnerable image from
being built, and ECR keeps looking after the fact. The first three are pull
request gates and the last is continuous, which matters because most
vulnerabilities are disclosed long after the code that carries them was written.

**What is still missing.** There is no `SAST` step (no CodeQL or similar), no
secret scanning or push protection, and no dependency audit for `billing`: the
Maven build runs `./mvnw verify` but no OWASP dependency check, so Java
dependencies are covered by Dependabot and by Trivy scanning the built image, but
not by a source-level auditor the way the other four languages are.

## 10. Backup and recovery

| What | Protection |
|---|---|
| RDS, production | 35 days of automated backups, point-in-time restore to any second in the window, `delete_automated_backups = false`, a final snapshot on destroy, `copy_tags_to_snapshot`, deletion protection on |
| RDS, staging | 7 days, deletion protection off, final snapshot skipped |
| S3 buckets | Versioning on both, noncurrent versions expiring after 180 days in production, `force_destroy = false` in production |
| Terraform state | Bucket versioning, so a truncated state file can be rolled back |
| ElastiCache | Three days of snapshots in production, though Redis holds nothing durable |
| Secrets Manager | 30 day recovery window in production, 0 in staging |
| ECR | The last twenty images per service, which is what makes rollback by image tag possible |

Backups are encrypted because the source is: a snapshot of an encrypted instance
is encrypted with the same key.

**Redis is deliberately disposable.** It holds the principal cache with a 60
second TTL, rate limiter counters and lockout counters. Losing it costs a few
seconds of extra database load and resets some lockout counters. Nothing needs
restoring.

**Recovery procedures** are in `RUNBOOK.md`: section 2 for rolling back a deploy
by image tag, section 4 for restoring RDS from a snapshot or to a point in time.
The restore procedure creates a **new** instance beside the current one and cuts
over after verification, because RDS cannot restore in place and because a
verified restore beside production is recoverable while a failed in-place attempt
is not.

**Recovery objectives.** Point-in-time restore gives a recovery point objective of
roughly five minutes. The recovery time objective is not a published number, and
should not be treated as one until it has been measured: `RUNBOOK.md` asks for a
quarterly rehearsal in staging on the grounds that an untested backup is not a
backup. A restore of a small database typically takes twenty minutes to an hour,
plus cutover.

**What is not backed up.** Nothing in a container: the services are stateless and
every image is rebuildable from the repository. Application configuration lives in
Terraform and in Secrets Manager rather than on a disk.

## 11. Out of scope

Controls a production deployment at a real freight forwarder would need, which
this system does not implement. They are listed rather than quietly omitted.

**Authentication and identity**

- **No multi-factor authentication.** A password plus lockout is the whole
  barrier. This is the single largest gap for a system holding payroll data.
- **No single sign-on.** No SAML or OIDC federation to a corporate directory, and
  no SCIM provisioning. Accounts are created and disabled in Bowline itself.
- **No password strength policy beyond a length minimum**, no breached-password
  check, no expiry.
- **No session management UI.** A user cannot see or revoke their own active
  sessions; an administrator ends them with `token_version` from the runbook.
- **No CAPTCHA or proof of work** on login. Lockout and rate limiting are the
  only anti-automation controls.

**Application and platform**

- **No WAF.** No AWS WAF in front of the ALB, so no managed rule sets, no IP
  reputation lists and no bot control. DDoS protection is Shield Standard, which
  is what every AWS account gets.
- **No SAST or secret scanning in the pipeline.** No CodeQL, no push protection.
  Dependency and image scanning *are* in place and blocking (section 9); source
  level static security analysis is not.
- **No source-level dependency audit for `billing`.** The other four languages
  have one; Java is covered only by Dependabot and by Trivy scanning the built
  image.
- **No penetration test and no bug bounty.** Nothing here has been tested by an
  adversary.
- **No runtime security.** No GuardDuty, no intrusion detection, no runtime
  container monitoring, no file integrity monitoring.
- **No distributed tracing.** `request_id` propagation is the substitute, which
  means correlating a request across services is a log search rather than a trace.

**Data**

- **No field-level encryption.** Salaries, addresses and payslip data are
  protected by the database roles and the authorisation model, not by encryption
  within the row. A read of the database file or a compromise of `bowline_app`
  yields plaintext.
- **No data retention or erasure policy.** The audit log is append-only and never
  pruned, and there is no implementation of a subject access or deletion request.
  A real deployment under GDPR needs both, and the append-only audit log is in
  genuine tension with a right to erasure.
- **No data residency controls.** Everything is in one region with no constraint
  preventing otherwise.
- **No log redaction pipeline.** Bodies are not logged, which is the control, but
  nothing scrubs a value that reaches a log through an error message.

**Operations and governance**

- **No automatic secret rotation.** Secrets Manager rotation schedules and Lambda
  rotators are not configured; rotation is the manual Terraform procedure in the
  runbook. Nothing forces it to happen on a cadence.
- **No customer managed KMS key for CloudWatch Logs by default.**
  `logs_kms_key_id` defaults to null, so log groups use CloudWatch's default
  encryption rather than a key the account controls.
- **No break-glass procedure and no separation of duties in AWS.** The deploy
  role is broad within `bowline-*`, and there is no second pair of eyes on an
  emergency change beyond the GitHub environment approval.
- **No compliance framework.** No SOC 2, ISO 27001 or PCI evidence, controls
  mapping or audit trail export.
- **No formal incident response plan**, no on-call rotation and no security
  contact. `RUNBOOK.md` covers operational incidents, not breaches.
- **Single tenant.** There is no tenant isolation, because there is exactly one
  company. Nothing in the schema or the authorisation model would keep two
  companies apart.

## 12. Reporting a problem

Bowline is a portfolio project and is not deployed anywhere serving real freight,
real employees or real money. There is no production instance to compromise and
no security contact rota. Security observations belong in a repository issue, and
findings about the design are as welcome as findings about the code.
