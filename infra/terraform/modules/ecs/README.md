# ecs

Fargate cluster `bowline-<env>` with Container Insights, the public load balancer, Cloud Map service discovery, IAM roles, task definitions and services for `api`, `web`, `billing`, `analytics` and `notify`, CPU autoscaling for `api` and `web`, and the one-off `bowline-migrate-<env>` task with its `run-task` network configuration in SSM.

## Routing

One hostname (`public_hostname`, for example `app.bowline.example`) serves both the web app and the API:

| Listener / rule                  | Match                                      | Target                        |
|----------------------------------|--------------------------------------------|-------------------------------|
| :80                              | everything                                 | 301 to https                  |
| :443 rule 5                      | `/metrics`                                 | fixed 404 (never public)      |
| :443 rule 10                     | `/api/*`, `/docs`, `/docs/*`, `/api-docs/*` | `api` target group, port 8080 |
| :443 default                     | everything else                            | `web` target group, port 3000 |

Both target groups health check `/healthz` every 15 seconds. Because web and API share an origin, `API_CORS_ORIGINS` and `API_PUBLIC_URL` are simply `https://<public_hostname>` and the browser never makes a cross-origin call.

Internally, `api`, `billing` and `analytics` register in the Cloud Map namespace `bowline.local` (A records, 10 s TTL), so the API reaches `http://billing.bowline.local:8081` and `http://analytics.bowline.local:8082`, and the web app's server side reaches `http://api.bowline.local:8080` without leaving the VPC. `notify` has no listener anyone calls; its `:9101` metrics port is reachable only inside the security group.

## Task definitions

Every variable name from `.env.example` is set, either as a plain value or as a secret pulled by the execution role at task start:

| Service     | Plain values                                                                                  | Secrets (Secrets Manager)                                                                     |
|-------------|-----------------------------------------------------------------------------------------------|------------------------------------------------------------------------------------------------|
| `api`       | `API_BIND`, `API_PUBLIC_URL`, `API_CORS_ORIGINS`, `JWT_ISSUER`, token TTLs, login limits, `RATE_LIMIT_PER_MINUTE`, `INVOICE_APPROVAL_THRESHOLD`, `DATABASE_MAX_CONNECTIONS`, `DATABASE_MIGRATE_ON_START=0`, `BILLING_URL`, `ANALYTICS_URL`, `LOG_FORMAT=json`, `RUST_LOG`, `S3_*`, `PRESIGN_TTL_SECONDS` | `DATABASE_URL` (db/app `url`), `REDIS_URL` (redis `url`), `JWT_SECRET`, `INTERNAL_SERVICE_TOKEN` |
| `web`       | `API_INTERNAL_URL`, `NEXT_PUBLIC_API_URL`, `NEXT_PUBLIC_APP_NAME`, `PORT`, `HOSTNAME`, `NODE_ENV` | none                                                                                       |
| `billing`   | `BILLING_BIND_PORT`, `BILLING_PDF_OUTPUT=s3`, company name and address, `S3_*`, `LOG_FORMAT` | `BILLING_DATABASE_URL` (db/ro `jdbc_url`), `BILLING_DATABASE_USER`, `BILLING_DATABASE_PASSWORD`, `INTERNAL_SERVICE_TOKEN` |
| `analytics` | `ANALYTICS_BIND_PORT`, `ANALYTICS_MODEL_PATH`, `LOG_FORMAT`                                    | `ANALYTICS_DATABASE_URL` (db/ro `url`), `INTERNAL_SERVICE_TOKEN`                             |
| `notify`    | `SMTP_HOST`, `SMTP_PORT`, `SMTP_STARTTLS=1`, `MAIL_FROM`, `NOTIFY_*`, `LOG_FORMAT`             | `DATABASE_URL_NOTIFY` (db/notify `url`), `SMTP_USERNAME`, `SMTP_PASSWORD`                    |
| `migrate`   | `LOG_FORMAT`, `RUST_LOG`, `DATABASE_MIGRATE_ON_START=1`                                        | `DATABASE_URL` (db/master `url`), `DATABASE_ROLE_PASSWORD_APP`, `_RO`, `_NOTIFY`             |

`S3_ENDPOINT`, `S3_ACCESS_KEY_ID` and `S3_SECRET_ACCESS_KEY` are set to empty strings: the SDKs then use the task role, which is the only credential the services have. `S3_FORCE_PATH_STYLE=0` because real S3 uses virtual-hosted addressing.

`extra_environment` adds or overrides plain values per service without editing the module, for example `{ api = { RUST_LOG = "debug" } }` in staging.

## IAM

| Role                          | Permissions                                                                                     |
|-------------------------------|-------------------------------------------------------------------------------------------------|
| `bowline-<env>-ecs-execution` | `AmazonECSTaskExecutionRolePolicy` (ECR pull, CloudWatch logs) plus `secretsmanager:GetSecretValue` on exactly the secrets above |
| `bowline-<env>-api-task`      | `Get/Put/DeleteObject`, `AbortMultipartUpload`, `ListBucket` on the documents and pdfs buckets; `kms:GenerateDataKey`, `kms:Decrypt` on the bucket key |
| `bowline-<env>-billing-task`  | `Get/PutObject`, `AbortMultipartUpload`, `ListBucket` on the pdfs bucket; same KMS grant         |
| `bowline-<env>-notify-task`   | `ses:SendEmail`, `ses:SendRawEmail` on the verified identity and configuration set             |
| `bowline-<env>-web-task`, `-analytics-task`, `-migrate-task` | nothing beyond the trust policy                                      |

All task roles additionally get the four `ssmmessages` actions when `enable_execute_command` is true, which is what `aws ecs execute-command` needs. The trust policy pins `aws:SourceAccount` and `aws:SourceArn` to this account's ECS.

## Deployments

Services run with `deployment_minimum_healthy_percent = 100`, `deployment_maximum_percent = 200` and the deployment circuit breaker with rollback: a task definition whose tasks keep failing health checks is rolled back automatically without operator action. `api` and `web` ignore `desired_count` after creation because target-tracking autoscaling (`ECSServiceAverageCPUUtilization`, target 60%, scale-out cooldown 60 s, scale-in 300 s) owns it between `min_count` and `max_count`.

## Migrations

`bowline-migrate-<env>` is a task definition, not a service. The deploy workflow runs it after `terraform apply`, waits for it to stop, and fails the deploy if the container's exit code is not zero:

```
aws ecs run-task --cluster bowline-<env> --task-definition bowline-migrate-<env> --launch-type FARGATE \
  --network-configuration "$(aws ssm get-parameter --name /bowline/<env>/migrate-network --query Parameter.Value --output text)"
```

The SSM parameter holds `{"awsvpcConfiguration":{"subnets":[...],"securityGroups":[...],"assignPublicIp":"DISABLED"}}`, so the workflow does not need to know subnet ids.

**The family name carries the environment on purpose.** Task definition families are account-wide, and `run-task` without a revision number resolves to the newest revision of the family. A single shared `bowline-migrate` would therefore receive revisions from both environments, and whichever was applied most recently would win: a staging deploy could run migrations against the production database, using the production master credential that the production revision references. Putting the environment in the family name makes that impossible rather than merely unlikely, so the safety no longer depends on the two environments being in separate accounts or on the workflow applying and migrating back to back.

`migrate_task_family` derives `bowline-migrate-<environment>` when left null, which is what the deploy workflow resolves (`FAMILY="bowline-migrate-$ENVIRONMENT"`). Setting it explicitly overrides the derivation, and the workflow has to be changed to match if you do.

## Inputs

See `variables.tf`; every variable carries a description. The required ones are `environment`, `vpc_id`, subnet and security group ids, `public_hostname`, `certificate_arn`, `ecr_registry`, `image_tag`, `log_group_names`, the secret ARNs, the bucket names, ARNs and KMS key, the SES ARNs, `smtp_host` and `mail_from`.

## Outputs

`cluster_name`, `cluster_arn`, `app_url`, `alb_dns_name`, `alb_zone_id`, `alb_arn`, `alb_arn_suffix`, `target_group_arn_suffixes`, `service_names`, `task_definition_arns`, `service_discovery_namespace`, `internal_urls`, `execution_role_arn`, `task_role_arns`, `migrate_task_family`, `migrate_task_definition_arn`, `migrate_network_parameter_name`.
