# Bowline runbook

The operational manual: how to deploy, how to undo a deploy, how to run
migrations, how to restore the database, how to rotate the two application
secrets, how to scale, where to look when something is wrong, what each alarm
means, and how to reproduce a production problem on a laptop.

`ARCHITECTURE.md` explains what the system is. `infra/README.md` explains how the
AWS account is put together and how to bootstrap it. This file assumes both are
already true and something needs doing right now.

## Conventions

- `<env>` is `staging` or `prod`. Every AWS resource is named for it: the ECS
  cluster is `bowline-<env>`, log groups are `/bowline/<env>/<service>`, secrets
  are `bowline/<env>/<name>`, alarms are `bowline-<env>-<what>`.
- `<sha>` is a twelve character commit SHA, which is also the image tag.
- Services are `api`, `web`, `billing`, `analytics`, `notify`. The one-off
  migration task is `bowline-migrate-<env>`.
- Examples use `000000000000` for the account and `bowline.example` for the
  domain. Substitute the real ones.
- Anything that changes production runs through the `prod` GitHub environment
  and needs an approval. The AWS CLI paths below exist for incidents, not for
  routine work.

Set these before pasting anything:

```
export AWS_REGION=us-east-1
export ENV=prod                 # or staging
export CLUSTER="bowline-${ENV}"
```

---

## 1. Deploying

The normal path is a push to `main`. `deploy.yml` then does three things in
order:

1. **images**: builds all five images with Buildx and pushes each to ECR twice,
   as `:<sha>` and as `:latest`. The `<sha>` tag is what gets deployed;
   `:latest` is only a convenience for humans.
2. **terraform**: waits for approval on the `prod` environment, then runs
   `terraform apply` in `infra/terraform/environments/<env>` with
   `-var image_tag=<sha>`. New task definition revisions are registered and the
   ECS services roll to them.
3. **migrate**: runs `bowline-migrate-<env>` as a one-off Fargate task, waits for
   it to stop, and fails the deploy if the container exit code is not zero.

Note the order: **the code is deployed before the migrations run.** That is only
safe because migrations are additive. A migration that drops or renames a column
the current code still reads will break production for the length of the rollout.
Ship such a change in two deploys: first a migration that adds the new shape and
code that writes both, then a later one that removes the old.

To deploy a specific environment on demand, use the `workflow_dispatch` trigger
and pick the environment. To watch a rollout:

```
aws ecs describe-services --cluster "$CLUSTER" \
  --services "bowline-${ENV}-api" "bowline-${ENV}-web" \
  --query 'services[].{name:serviceName,desired:desiredCount,running:runningCount,rollout:deployments[0].rolloutState}'
```

`rolloutState` moves `IN_PROGRESS` to `COMPLETED`. Both public services run with
`deployment_minimum_healthy_percent = 100` and `maximum_percent = 200`, so new
tasks come up and pass `/healthz` before old ones go away and there is no
capacity dip. The deployment circuit breaker is on with `rollback = true`: if the
new tasks never become healthy, ECS stops the rollout and puts the previous task
definition back on its own. A rollout that ends `ROLLED_BACK` means the new image
is broken, not that the deploy system is.

If a rollout stalls, the reason is almost always in the service events:

```
aws ecs describe-services --cluster "$CLUSTER" --services "bowline-${ENV}-api" \
  --query 'services[0].events[0:10].message' --output text
```

The three usual causes are an image tag that does not exist in ECR, an execution
role that cannot read a secret (check the secret ARN in the task definition), and
a container that starts and then fails `/healthz` because it cannot reach the
database.

---

## 2. Rolling back to a previous image tag

Every deploy is identified by the image tag, and every previous tag is still in
ECR (the lifecycle policy keeps the last twenty images per repository). Rolling
back is applying the environment with an older tag. There is no separate rollback
mechanism to get wrong.

**Find the tag to go back to.** Either take it from the previous successful run
of `deploy.yml`, or list what is in the registry:

```
aws ecr describe-images --repository-name bowline/api \
  --query 'reverse(sort_by(imageDetails,&imagePushedAt))[0:10].{tag:imageTags[0],pushed:imagePushedAt}' \
  --output table
```

**Roll back.** Preferred, because it keeps Terraform state and reality in
agreement:

```
cd infra/terraform/environments/${ENV}
terraform init
terraform plan  -var "image_tag=<previous sha>"
terraform apply -var "image_tag=<previous sha>"
```

The plan should show changes to the five task definitions and the services and
nothing else. If it wants to change a database, a subnet or a security group,
stop: someone has changed infrastructure outside the workflow and rolling back
would revert that too.

**The fast path**, when the site is down and Terraform is not to hand: point the
services at the previous task definition revision directly.

```
aws ecs update-service --cluster "$CLUSTER" \
  --service "bowline-${ENV}-api" \
  --task-definition "bowline-${ENV}-api:<previous revision>" \
  --force-new-deployment
```

This drifts from Terraform state. Follow it with a proper `apply` at the old tag
as soon as the incident is over, or the next deploy will silently undo it.

**Rolling back a migration is a different problem.** The migrations are forward
only. If the previous image cannot read the current schema, rolling the image
back is not enough and you are into section 4 or section 5. This is the reason
migrations must be additive.

---

## 3. Database migrations

Migrations live in `db/migrations` as numbered SQL files and are applied by the
`migrate` binary from the `api` image. In production they never run inside a
service: `DATABASE_MIGRATE_ON_START=0` is set on the `api` task definition, so
the API refuses to migrate on boot and only checks that the schema is current
when answering `/readyz`. The dedicated `bowline-migrate-<env>` task has
`DATABASE_MIGRATE_ON_START=1`.

The migrate task is the only thing in the system that holds the RDS master
credential. On its first run it uses that credential to create `bowline_app`,
`bowline_ro` and `bowline_notify` with the passwords Terraform generated, which
it receives as `DATABASE_ROLE_PASSWORD_APP`, `_RO` and `_NOTIFY`. After that it
just applies pending files.

**Run it:**

```
TASK_ARN="$(aws ecs run-task --cluster "$CLUSTER" \
  --task-definition "bowline-migrate-${ENV}" --launch-type FARGATE \
  --network-configuration "$(aws ssm get-parameter \
     --name "/bowline/${ENV}/migrate-network" \
     --query Parameter.Value --output text)" \
  --query 'tasks[0].taskArn' --output text)"
```

The SSM parameter holds the subnets and security group as JSON so the command
does not have to hard-code ids that change with every VPC rebuild.

**Confirm it worked.** Wait for the task to stop, then read the exit code:

```
aws ecs wait tasks-stopped --cluster "$CLUSTER" --tasks "$TASK_ARN"

aws ecs describe-tasks --cluster "$CLUSTER" --tasks "$TASK_ARN" \
  --query 'tasks[0].{status:lastStatus,exit:containers[0].exitCode,reason:stoppedReason}'
```

`exitCode` 0 is success. Anything else, read the log:

```
aws logs tail "/bowline/${ENV}/migrate" --since 15m
```

**If a migration fails halfway.** Each file is applied in its own transaction, so
a failure leaves the database at the last complete file. Fix the SQL, push, and
run the task again; already-applied files are skipped. The exception is a
migration that fails on data rather than syntax, for example a `not null` added
to a column with existing nulls. In that case write the backfill as an earlier
migration and redeploy, rather than fixing rows by hand and pretending the file
was fine.

**Never do this:** do not run `psql` against production and apply a file
manually. The migration tracking table will not know about it and the next
deployment will try to apply it again.

**Why the family name has the environment in it.** Task definition families are
account-wide, and `run-task` given a family with no revision number resolves to
the newest revision of that family. If both environments shared one
`bowline-migrate` family, whichever was applied most recently would win, and a
staging deploy could run migrations against production using the production
master credential embedded in that revision. The family is therefore
`bowline-migrate-<env>`, derived from the same environment name in both the `ecs`
module and the workflow, so the two cannot drift. Use `"$CLUSTER"` and
`"bowline-migrate-${ENV}"` together and the pair is always consistent.

It still costs nothing to confirm what you are about to run:

```
aws ecs describe-task-definition --task-definition "bowline-migrate-${ENV}" \
  --query 'taskDefinition.{family:family,rev:revision,role:taskRoleArn}'
```

The role ARN must contain the environment you think you are in.

---

## 4. Restoring from an RDS snapshot

Production keeps 35 days of automated backups with a five minute recovery point
objective through point-in-time restore; staging keeps 7. Automated backups are
taken in the 03:00 to 04:00 UTC window and are not deleted when the instance is
(`delete_automated_backups = false`), and a manual final snapshot is taken on
destroy.

**Restoring is never in place.** RDS restores into a *new* instance. The
sequence is: restore beside the current one, check it, then move traffic.

**1. Find the restore point.**

```
# Automated snapshots
aws rds describe-db-snapshots --db-instance-identifier "bowline-${ENV}-postgres" \
  --snapshot-type automated \
  --query 'reverse(sort_by(DBSnapshots,&SnapshotCreateTime))[0:10].{id:DBSnapshotIdentifier,at:SnapshotCreateTime}' \
  --output table

# Or the window available for point-in-time restore
aws rds describe-db-instances --db-instance-identifier "bowline-${ENV}-postgres" \
  --query 'DBInstances[0].{earliest:LatestRestorableTime,latest:LatestRestorableTime}'
```

**2. Restore to a new instance.** Point-in-time is usually what you want after a
bad migration or a bad delete, because it can land at the second before the
damage:

```
aws rds restore-db-instance-to-point-in-time \
  --source-db-instance-identifier "bowline-${ENV}-postgres" \
  --target-db-instance-identifier "bowline-${ENV}-postgres-restore" \
  --restore-time 2026-08-27T09:14:00Z \
  --db-subnet-group-name "bowline-${ENV}-db" \
  --vpc-security-group-ids <the bowline-<env>-db security group id> \
  --db-parameter-group-name "bowline-${ENV}-postgres16" \
  --no-publicly-accessible
```

From a named snapshot, use `restore-db-instance-from-db-snapshot` with
`--db-snapshot-identifier` instead of the source and time. In both cases pass the
subnet group, security group and parameter group explicitly: the defaults put the
new instance somewhere reachable, which is exactly what the isolated subnet tier
exists to prevent.

**3. Wait and verify.** Twenty minutes to an hour depending on size.

```
aws rds wait db-instance-available --db-instance-identifier "bowline-${ENV}-postgres-restore"
```

Connect through a task with ECS Exec (section 9) and check the data is what you
expect. The restored instance has the master password from the moment of the
snapshot, and the three application roles exist inside it because they are
schema objects, not RDS configuration.

**4. Cut over.** Two options.

*Rename*, which keeps the endpoint hostname and so needs no Terraform change to
the services, but does confuse Terraform state:

```
aws rds modify-db-instance --db-instance-identifier "bowline-${ENV}-postgres" \
  --new-db-instance-identifier "bowline-${ENV}-postgres-old" --apply-immediately
aws rds modify-db-instance --db-instance-identifier "bowline-${ENV}-postgres-restore" \
  --new-db-instance-identifier "bowline-${ENV}-postgres" --apply-immediately
```

Afterwards, `terraform import` the renamed instance over
`module.database.aws_db_instance.this` and run a plan until it is empty. Until
that is done, do not let the deploy workflow run: it would try to recreate the
database.

*Or repoint*, which is cleaner but slower: change the database module to use the
restored instance, apply, and let the task definitions pick up the new endpoint
from the regenerated secrets.

Either way, stop the services first so nothing writes to the old instance during
the swap:

```
for s in api billing analytics notify; do
  aws ecs update-service --cluster "$CLUSTER" --service "bowline-${ENV}-${s}" --desired-count 0
done
```

Bring them back up after the cutover, then run the migrate task, because the
restored database may be behind the deployed code.

**5. Clean up.** Delete the old instance only after a full working day of normal
operation, and take a final snapshot when you do.

**Test this.** A restore that has never been rehearsed is not a backup. Restore
staging from a snapshot once a quarter and time it; that number is the real
recovery time objective.

---

## 5. Rotating `JWT_SECRET`

`JWT_SECRET` signs the HS256 access tokens. It lives in Secrets Manager at
`bowline/<env>/jwt-secret` and is generated by `random_id.jwt_secret` in the
`secrets` module.

**What rotation costs users.** Access tokens are signed with the old key and are
valid for fifteen minutes. The moment the API starts with a new key, every
outstanding access token fails validation. Refresh tokens are *not* JWTs: they
are opaque random bytes stored hashed in `refresh_tokens`, so they survive
rotation untouched. The web app refreshes automatically on a 401. The practical
effect is therefore a single failed request per active user, retried
transparently, and nobody is logged out.

**Rotate:**

```
cd infra/terraform/environments/${ENV}
terraform apply -replace='module.secrets.random_id.jwt_secret' -var "image_tag=<current sha>"
```

That regenerates the value, writes a new secret version and, because the secret
ARN itself does not change, leaves the task definitions alone. The running tasks
have already read the old value into their environment, so they must be
restarted to pick it up:

```
for s in api; do
  aws ecs update-service --cluster "$CLUSTER" --service "bowline-${ENV}-${s}" --force-new-deployment
done
```

Only `api` signs or verifies access tokens, so only `api` needs the restart.

**Rotate immediately if** a JWT secret has been printed into a log, committed,
pasted into a ticket, or if an operator with access to Secrets Manager has left.
Rotating invalidates every access token minted with the old key, which is the
point.

### The bigger hammer: `token_version`

Rotating the signing key does not end sessions, because refresh tokens still
work. Every access token carries a `tv` claim which must equal the user's
`token_version` column, checked on every request when the principal is loaded.
Incrementing it invalidates that user's tokens *and* their cached principal.

The API does this itself when a password changes and when an employee is
terminated. To force a single user out of every session immediately:

```sql
update users set token_version = token_version + 1 where email = 'someone@bowline.example';
delete from refresh_tokens where user_id = (select id from users where email = 'someone@bowline.example');
```

To log out the entire company, for example after a suspected token theft:

```sql
update users set token_version = token_version + 1;
delete from refresh_tokens;
```

That is a genuine outage for every logged-in user: they land on the login screen
on their next request. Do it deliberately, and tell people first if you can. The
principal cache in Redis holds entries for 60 seconds, so the effect is complete
within a minute.

---

## 6. Rotating the internal service token

`INTERNAL_SERVICE_TOKEN` is the shared secret the API sends as `X-Internal-Token`
on every call to `billing` and `analytics`. It is at
`bowline/<env>/internal-service-token`, generated by
`random_password.internal_service_token`.

**This one has a window.** Three services read the same value: `api` sends it,
`billing` and `analytics` check it. Restart them in the wrong order and calls
fail in between. Because the consumers are the ones that reject, restart the
consumers first: a `billing` that already knows both values is impossible, so
what actually happens is that `billing` and `analytics` come up on the new token
and briefly reject the old `api`, then `api` comes up on the new token and the
gap closes.

```
cd infra/terraform/environments/${ENV}
terraform apply -replace='module.secrets.random_password.internal_service_token' -var "image_tag=<current sha>"

# consumers first
for s in billing analytics; do
  aws ecs update-service --cluster "$CLUSTER" --service "bowline-${ENV}-${s}" --force-new-deployment
done
aws ecs wait services-stable --cluster "$CLUSTER" \
  --services "bowline-${ENV}-billing" "bowline-${ENV}-analytics"

# then the caller
aws ecs update-service --cluster "$CLUSTER" --service "bowline-${ENV}-api" --force-new-deployment
```

**What breaks during the gap.** `analytics` supplies the shipment delay-risk
score, and the API is built to fail open on it: a missing score never blocks a
booking, the shipment is simply stored without one. `billing` renders invoice
PDFs, and that does not fail open: issuing an invoice during the window will
return an error to the accountant. Rotate outside business hours, or accept that
a handful of invoice issues need retrying. Nothing is left half-written, because
the PDF is rendered after the ledger entry is committed and the key is recorded
separately.

`notify` and `web` do not use this token and need no restart.

---

## 7. Scaling services

**`api` and `web` autoscale.** Both sit behind the load balancer and have a CPU
target-tracking policy at 60 percent average utilisation, scaling out after 60
seconds above target and back in after 300 seconds below it. In production `api`
runs 2 to 6 tasks and `web` runs 2 to 4. Terraform sets the initial
`desired_count` and then ignores it (`lifecycle { ignore_changes = [desired_count] }`),
so autoscaling owns the number afterwards and a deploy will not yank it back down.

To change the range, edit `services` in the environment's `terraform.tfvars` and
apply:

```hcl
services = {
  api       = { cpu = 1024, memory = 2048, desired_count = 2, min_count = 3, max_count = 10 }
  web       = { cpu = 512,  memory = 1024, desired_count = 2, min_count = 2, max_count = 4 }
  billing   = { cpu = 1024, memory = 2048, desired_count = 1 }
  analytics = { cpu = 1024, memory = 2048, desired_count = 1 }
  notify    = { cpu = 256,  memory = 512,  desired_count = 1 }
}
```

`cpu` and `memory` are Fargate units and only certain pairs are legal (256 with
512, 512 with 1024, 1024 with 2048 and upwards). Changing them registers a new
task definition revision and rolls the service.

**To scale right now, ahead of a known spike**, raise the floor rather than the
desired count, because autoscaling would otherwise pull it straight back:

```
aws application-autoscaling register-scalable-target \
  --service-namespace ecs --scalable-dimension ecs:service:DesiredCount \
  --resource-id "service/${CLUSTER}/bowline-${ENV}-api" \
  --min-capacity 4 --max-capacity 10
```

Put it back afterwards, or the next `terraform apply` will and you will have
forgotten why capacity dropped.

**`billing`, `analytics` and `notify` do not autoscale.** They have no load
balancer and their work is either bursty and short (`billing`) or a single poller
(`notify`). Scale them by changing `desired_count` and applying.

**`notify` must stay at one task in normal operation.** More than one is safe,
because the worker claims rows with `SELECT ... FOR UPDATE SKIP LOCKED`, but two
workers double the polling load on the database for no gain unless the outbox is
genuinely backed up. See section 10 for when raising it is the right call.

**Scaling the database.** `api` opens up to `DATABASE_MAX_CONNECTIONS`
connections per task, 40 in production. Six `api` tasks at full stretch is 240
connections, plus `billing`, `analytics` and `notify`. Check the instance class
can take it before raising `max_count`, and remember that
`apply_immediately = false` in production means an instance class change waits
for the Sunday 04:30 UTC maintenance window unless you force it.

---

## 8. Where the logs and metrics live

**Logs.** Every container ships stdout to CloudWatch Logs with the `awslogs`
driver. One group per service:

| Group                          | Contents                                       |
|--------------------------------|------------------------------------------------|
| `/bowline/<env>/api`           | Rust `tracing` JSON, one line per request       |
| `/bowline/<env>/web`           | Next.js server output                          |
| `/bowline/<env>/billing`       | Spring Boot                                    |
| `/bowline/<env>/analytics`     | FastAPI and the scoring calls                  |
| `/bowline/<env>/notify`        | The outbox worker, including a heartbeat line  |
| `/bowline/<env>/migrate`       | One stream per migration task run              |
| `/bowline/<env>/vpc-flow-logs` | Accepted and rejected connections in the VPC   |

Retention is 90 days in production and 14 in staging.

Every API log line carries `request_id`, and the same id comes back to the client
in the response, so a user's bug report with an id is directly greppable:

```
aws logs tail "/bowline/${ENV}/api" --since 1h --follow \
  --filter-pattern '{ $.request_id = "01J8Z..." }'
```

Useful filters:

```
# errors only
aws logs tail "/bowline/${ENV}/api" --since 30m --filter-pattern '{ $.level = "ERROR" }'

# everything one user did
aws logs tail "/bowline/${ENV}/api" --since 6h --filter-pattern '{ $.user_id = "<uuid>" }'

# slow requests
aws logs tail "/bowline/${ENV}/api" --since 1h --filter-pattern '{ $.latency_ms > 1000 }'
```

**Metrics.** Three sources, and it matters which one you are looking at.

- *Prometheus, per service.* Every service exposes `/metrics`: request counts and
  latency histograms, database pool statistics, outbox depth. These are inside
  the VPC only. The ALB has an explicit rule at priority 5 returning a fixed 404
  for `/metrics`, so it can never be reached from the internet. To read them,
  exec into a task (below) and `curl localhost:8080/metrics`.
- *CloudWatch, from AWS.* `AWS/ApplicationELB` for request counts, target
  response time and 5xx; `AWS/RDS` for CPU, connections, free storage and read
  and write latency; `AWS/ElastiCache` for cache CPU, evictions and hit rate;
  `ECS/ContainerInsights` for per-service running and desired task counts, CPU
  and memory utilisation.
- *`Bowline/<env>`, derived from logs.* One metric, `OutboxDepth`, extracted from
  the `notify` heartbeat by a metric filter on `$.outbox_depth`.

**Traces.** There is no distributed tracing. `request_id` propagates through the
API's own calls and is the substitute; correlating an API request with the
`billing` call it made means grepping both groups for the same id.

**Getting a shell.** ECS Exec is on in staging and off in production
(`enable_execute_command`). To turn it on for an incident, set the variable and
apply, which rolls the services. Then:

```
aws ecs execute-command --cluster "$CLUSTER" \
  --task <task id> --container api --interactive --command "/bin/sh"
```

Turn it back off when the incident is closed. Every session is recorded in
CloudTrail.

---

## 9. Alarms

Eleven alarms per environment, all publishing to the SNS topic
`bowline-<env>-alarms`. The email address in `alarm_email` is subscribed and must
confirm by clicking the link in the confirmation mail; until it does, alarms fire
into nothing. Add PagerDuty or Slack as further subscriptions on the same topic.
Every alarm also sends an OK notification, so a resolved alarm tells you so.

### `bowline-<env>-alb-5xx-rate`

Target 5xx responses exceeded 5 percent of requests for two minutes out of three.
This is a metric maths alarm over `HTTPCode_Target_5XX_Count` divided by
`RequestCount`, so it does not fire on a single error during a quiet night.

Note *Target* 5xx: these are errors the application returned, not errors the load
balancer generated. If the ALB itself is failing you will see
`HTTPCode_ELB_5XX_Count` instead, which usually means no healthy targets and the
unhealthy-targets alarm will be firing too.

Do: check whether a deploy just finished, then look at the API error log
(`{ $.level = "ERROR" }`). A burst of `503` with database errors underneath means
the connection pool is exhausted or RDS is unwell; check `rds-cpu` and the RDS
`DatabaseConnections` metric. If it started with a deploy and does not settle
within a few minutes, roll back (section 2).

### `bowline-<env>-api-unhealthy-targets` and `bowline-<env>-web-unhealthy-targets`

At least one task failed `/healthz` for three minutes.

Do: `describe-services` and read the events. `/healthz` only says the process is
up, so a failure here means the container is crashing, out of memory, or never
finished starting. Check the task's `stoppedReason` for `OutOfMemoryError`, and
the log group for a panic. If tasks are cycling, the deployment circuit breaker
should already have rolled back; if it has not, the tasks are passing health
checks briefly and then dying, which points at memory.

A related failure this alarm does *not* catch: a task that is up and healthy but
cannot reach the database answers `/healthz` fine and `/readyz` with an error.
The ALB health check uses `/healthz` deliberately, so that a database blip does
not take every task out of service at once and turn a slow database into a total
outage. Watch `readyz` through the API's own metrics and the error logs.

### `bowline-<env>-rds-cpu`

Average CPU above 80 percent for fifteen minutes.

Do: open Performance Insights (enabled in production, seven days of history) and
sort by top SQL. The usual culprits are a missing index after a new query
shipped, a report view being run interactively over a large period, or a
connection storm from `api` tasks that just scaled out. Check
`DatabaseConnections` against the instance limit. Short term, scale `api` back
in; longer term, add the index. Do not resize the instance during an incident:
in production `apply_immediately` is false, and forcing it causes a Multi-AZ
failover of about a minute.

### `bowline-<env>-rds-free-storage`

Free storage under 10 GiB in production, 5 GiB in staging.

Do: storage autoscaling should be growing the volume already, up to
`max_allocated_storage_gb` (500 in production). If it has hit that ceiling, raise
it and apply. If growth is unexpected, the usual causes are the audit log (which
is append-only and never pruned by design), a runaway `notifications` table, or
WAL held by a long-running transaction. Check the largest relations:

```sql
select relname, pg_size_pretty(pg_total_relation_size(c.oid)) as size
  from pg_class c join pg_namespace n on n.oid = c.relnamespace
 where n.nspname = 'public' and c.relkind = 'r'
 order by pg_total_relation_size(c.oid) desc limit 15;
```

Never delete from `audit_log`. A trigger rejects it, and that is deliberate. If
it genuinely must be trimmed for storage reasons, that is a schema change with an
archive table, reviewed like any other, not a `delete` in a production shell.

### `bowline-<env>-<service>-running-below-desired`

One alarm per service. Running task count stayed below desired for five
consecutive minutes, which is longer than any healthy rolling deployment.

Do: read the service events for the stopped-task reason. In order of likelihood:
the image tag does not exist in ECR (a deploy raced ahead of the image push); the
execution role cannot read a secret (`ResourceInitializationError` in the stopped
reason, usually after a secret was replaced and its ARN changed); no capacity in
the availability zone; the container exits immediately on a configuration error.
The last one shows up in the log group with a startup error rather than in the
ECS events.

### `bowline-<env>-outbox-depth`

More than 500 notifications pending for fifteen minutes. See section 10, which is
about this alarm entirely.

### Alarms that do not exist

Worth knowing, so nobody assumes coverage that is not there: there is no alarm on
p99 latency, on SES bounce or complaint rate, on certificate expiry, on the
Redis cache, or on failed logins. SES bounce and complaint rates in particular
are worth watching manually in the SES console, because a bad rate gets the
sending domain suspended and no alarm here would warn you.

---

## 10. Inspecting and draining the notification outbox

Every message, announcement and ticket update writes a row into `notifications`
inside the same transaction as the business change. The `notify` worker polls
every two seconds, claims up to 50 rows with `SELECT ... FOR UPDATE SKIP LOCKED`,
sends each through SES SMTP, and retries with exponential backoff. After eight
attempts a row is parked as `failed` and left alone. Nothing is ever lost if the
mail provider is down; it just accumulates.

Rows are `pending`, `sending`, `sent` or `failed`. Depth means `pending` plus
`sending`.

**Quick look:**

```
DATABASE_URL_NOTIFY=<the notify role url> bowctl outbox depth
```

which prints the depth, the sent count and how many are parked as failed.

**Detail**, connecting as `bowline_notify` (it can select and update
`notifications`, and nothing else in the database):

```sql
-- Where the backlog is
select status, count(*), min(created_at) as oldest
  from notifications group by status order by status;

-- What is actually failing, grouped by cause
select attempts, left(last_error, 120) as error, count(*)
  from notifications
 where status = 'failed'
 group by attempts, left(last_error, 120)
 order by count(*) desc limit 20;

-- Rows stuck retrying rather than parked
select id, to_address, subject, attempts, next_attempt_at, left(last_error, 160)
  from notifications
 where status = 'pending' and attempts > 0
 order by next_attempt_at limit 50;
```

**Diagnosing a rising depth.** Three causes, distinguishable in seconds:

1. *The worker is not running.* Depth rises steadily and the `notify` log group
   has gone quiet. Check `running-below-desired` for `notify` and the service
   events. This is the common case.
2. *SES is rejecting.* The worker is running and logging, `last_error` is
   populated on a growing number of rows. Look at the message. A sandbox account
   can only send to verified addresses, which is the classic staging surprise.
   `Maximum sending rate exceeded` means the account limit is below what a
   company-wide broadcast needs. Bounces and complaints show up in the SES
   reputation metrics on the `bowline-<env>` configuration set.
3. *A broadcast is draining normally.* Depth jumped by roughly 260 (one row per
   employee) and is falling steadily. Nothing to do. At 50 rows per two second
   poll a full company announcement clears in about twelve seconds, so if this is
   still alarming after fifteen minutes it is not case 3.

**Draining a backlog faster.** The worker is safe to run more than once, because
`SKIP LOCKED` means two workers never claim the same row:

```
aws ecs update-service --cluster "$CLUSTER" --service "bowline-${ENV}-notify" --desired-count 3
```

Put it back to 1 once the depth is near zero. Do not go past three or four: the
limit is nearly always the SES sending rate, not the worker, and more pollers
just add database load.

**Retrying parked rows.** After fixing whatever was rejecting them, reset the
`failed` rows and the worker will pick them up on its next poll:

```sql
-- Look before you leap
select count(*), min(created_at), max(created_at) from notifications where status = 'failed';

-- Retry a specific batch
update notifications
   set status = 'pending', attempts = 0, next_attempt_at = now(), last_error = null
 where status = 'failed'
   and created_at > now() - interval '24 hours';
```

Scope it by time or by error. Resetting every `failed` row that has ever existed
will re-send weeks-old notifications to people who have long since dealt with
them, and some of those addresses are why the rows failed in the first place,
which harms the domain's sending reputation.

**Abandoning rows.** If a batch is genuinely undeliverable, for example messages
to a terminated employee's address, leave them as `failed`. That is the archive.
Deleting them loses the record that the system tried.

---

## 11. Reproducing a production issue locally

The local stack is the same containers with the same environment variable names.
The differences are deliberate and worth holding in mind while debugging: MinIO
instead of S3, Mailpit instead of SES, one Postgres container instead of Multi-AZ
RDS, and `LOG_FORMAT=pretty` instead of JSON.

**1. Get the stack up.**

```
cp .env.example .env
make up          # postgres, redis, mailpit, minio
make migrate
make seed        # the 260-person company
make api         # :8080
make web         # :3000
```

Log in as `ceo@bowline.example` with the password in `SEED_PASSWORD`. The seed
gives every user the same password and sets `SEED_SKIP_PASSWORD_CHANGE=1`, which
is precisely why the seed must never run against production.

Or run everything in containers, which is closer to production because the
services talk to each other by service name rather than through localhost:

```
docker compose --profile app up -d --build
```

**2. Match production's shape where it matters.** Most behaviour differences come
down to a handful of variables in `.env`:

```
LOG_FORMAT=json                       # so log lines look like production's
RUST_LOG=info,bowline_api=info,sqlx=warn
DATABASE_MIGRATE_ON_START=0           # production does not migrate on boot
INVOICE_APPROVAL_THRESHOLD=50000
RATE_LIMIT_PER_MINUTE=300
ACCESS_TOKEN_TTL_SECONDS=900
```

If the bug involves authorisation, the seed data is the point: it builds the same
seven-level hierarchy with the same roles, so "a supervisor cannot see X" is
reproducible by logging in as one.

**3. Reproduce with the same request.** Take `request_id` from the report, find
the line in CloudWatch, and rebuild the call. The API's OpenAPI document is at
`http://localhost:8080/docs`.

**4. Copy production data only if you must, and sanitise it.** The bugs that need
real data are almost always finance or hierarchy shaped. If you take an export,
it contains employee names, addresses, salaries and customer commercial terms:
treat it as production data on your laptop, work in a disposable database, and
delete it when you are done. Prefer writing a failing test against seeded data,
which is also what stops the bug coming back.

**5. Things that will not reproduce locally.** Anything about IAM, presigned URL
signing against real S3, SES delivery and bounces, ALB routing and the `/metrics`
404 rule, Cloud Map resolution, Multi-AZ failover, or autoscaling. For those,
staging is the reproduction environment, and it is built from the same modules as
production for exactly that reason.

**6. The end-to-end check.** `scripts/smoke.sh` walks the whole scenario against
the local stack: the CEO logs in and broadcasts, a dock worker opens a ticket, an
agent resolves it, a coordinator books a shipment, an accountant issues the
invoice, and the ledger balances. If it passes and production does not, the
difference is environmental, and that narrows the search considerably.

```
make smoke
```
