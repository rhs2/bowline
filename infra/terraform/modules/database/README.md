# database

RDS PostgreSQL 16 for one environment: encrypted gp3 storage, optional Multi-AZ, automated backups (7 to 35 days), deletion protection, Performance Insights, enhanced monitoring, a parameter group that logs slow statements and forces TLS, and every credential in Secrets Manager.

## What it creates

- `aws_db_instance` `bowline-<env>-postgres` in the isolated subnets, never publicly accessible.
- Parameter group `bowline-<env>-postgres16` with `log_min_duration_statement` (default 500 ms), `rds.force_ssl = 1` and `log_statement = ddl`. The `postgresql` and `upgrade` logs are exported to CloudWatch under `/aws/rds/instance/bowline-<env>-postgres/`.
- Enhanced monitoring role when `monitoring_interval > 0`.
- Secrets Manager secrets, all with the same JSON layout (`engine`, `host`, `port`, `dbname`, `username`, `password`, `url`, `jdbc_url`):

| Secret                         | Role             | Consumer                          |
|--------------------------------|------------------|-----------------------------------|
| `bowline/<env>/db/master`      | `bowline_admin`  | `bowline-migrate-<env>` task only       |
| `bowline/<env>/db/app`         | `bowline_app`    | `api` (`DATABASE_URL`)            |
| `bowline/<env>/db/ro`          | `bowline_ro`     | `billing`, `analytics`            |
| `bowline/<env>/db/notify`      | `bowline_notify` | `notify` (`DATABASE_URL_NOTIFY`)  |

The `url` values carry `?sslmode=require`; combined with `rds.force_ssl` no plaintext connection is possible.

## Application roles are created by the migrate task

RDS only creates the master user. Terraform deliberately does not connect to PostgreSQL (that would need a network path from the CI runner into the isolated subnets). Instead the `bowline-migrate-<env>` ECS task, which runs the `api` image with the `migrate` command inside the VPC, receives:

- `DATABASE_URL` from `bowline/<env>/db/master`
- `DATABASE_ROLE_PASSWORD_APP`, `DATABASE_ROLE_PASSWORD_RO`, `DATABASE_ROLE_PASSWORD_NOTIFY` from the three role secrets

and, before applying `db/migrations/*.sql`, runs the RDS adaptation of `db/init/roles.sql`. The local file uses `create role ... login createdb`, `create database` and `\connect`, none of which apply on RDS (the database already exists, `createdb` is unnecessary, and there is no psql). The equivalent that the migrate task executes is:

```sql
-- idempotent: safe on every deploy
do $$
begin
  if not exists (select 1 from pg_roles where rolname = 'bowline_app') then
    create role bowline_app login;
  end if;
  if not exists (select 1 from pg_roles where rolname = 'bowline_ro') then
    create role bowline_ro login;
  end if;
  if not exists (select 1 from pg_roles where rolname = 'bowline_notify') then
    create role bowline_notify login;
  end if;
end $$;

alter role bowline_app    password :'app_password';
alter role bowline_ro     password :'ro_password';
alter role bowline_notify password :'notify_password';

-- the master user must be a member of a role to hand it ownership on RDS
grant bowline_app to bowline_admin;
alter database bowline owner to bowline_app;
grant all on schema public to bowline_app;
alter default privileges for role bowline_app in schema public grant select on tables    to bowline_ro;
alter default privileges for role bowline_app in schema public grant select on sequences to bowline_ro;
```

Passwords are passed as bind parameters, never interpolated into SQL text. The grants that `bowline_notify` needs on the `notifications` table are part of the migrations themselves (`db/migrations/0007_audit_outbox.sql`), as are the `bowline_ro` grants on existing tables.

Rotating a role password is `terraform apply -replace='module.database.random_password.role["app"]'` followed by a migrate run (which re-applies the `alter role`) and a redeploy of the consuming service so it picks up the new secret value. See `docs/RUNBOOK.md`.

## Sizing guidance

| Environment | Class            | Multi-AZ | Backups | Deletion protection |
|-------------|------------------|----------|---------|---------------------|
| staging     | `db.t4g.medium`  | no       | 7 days  | off                 |
| prod        | `db.m7g.large`   | yes      | 35 days | on                  |

Storage autoscaling grows the volume up to `max_allocated_storage_gb`; the observability module alarms when free space drops under 5 GiB.

## Inputs

| Name                                  | Type         | Default          |
|---------------------------------------|--------------|------------------|
| `environment`                         | string       |                  |
| `subnet_ids`                          | list(string) |                  |
| `security_group_ids`                  | list(string) |                  |
| `instance_class`                      | string       | `db.t4g.medium`  |
| `engine_version`                      | string       | `16`             |
| `parameter_group_family`              | string       | `postgres16`     |
| `allocated_storage_gb`                | number       | `50`             |
| `max_allocated_storage_gb`            | number       | `200`            |
| `multi_az`                            | bool         | `false`          |
| `backup_retention_days`               | number       | `7`              |
| `deletion_protection`                 | bool         | `true`           |
| `skip_final_snapshot`                 | bool         | `false`          |
| `performance_insights_enabled`        | bool         | `true`           |
| `performance_insights_retention_days` | number       | `7`              |
| `monitoring_interval`                 | number       | `60`             |
| `log_min_duration_statement_ms`       | number       | `500`            |
| `database_name`                       | string       | `bowline`        |
| `master_username`                     | string       | `bowline_admin`  |
| `kms_key_id`                          | string       | `null`           |
| `apply_immediately`                   | bool         | `false`          |
| `secret_recovery_window_days`         | number       | `7`              |
| `secrets_kms_key_id`                  | string       | `null`           |
| `tags`                                | map(string)  | `{}`             |

## Outputs

`endpoint`, `port`, `database_name`, `instance_identifier`, `instance_arn`, `engine_version_actual`, `master_username`, `master_secret_arn`, `master_secret_name`, `role_names`, `role_secret_arns`, `role_secret_names`, `parameter_group_name`.
