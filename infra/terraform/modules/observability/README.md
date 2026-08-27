# observability

CloudWatch log groups, alarms and the SNS topic they notify, for one environment.

## Log groups

`/bowline/<env>/<service>` for `api`, `web`, `billing`, `analytics`, `notify` and `migrate`, with `log_retention_days` retention (30 by default). The ecs module points each task's `awslogs` driver at these names, so a service's logs are always under one group with one stream per task. Services log JSON (`LOG_FORMAT=json`), which makes CloudWatch Logs Insights queries such as `filter request_id = "..."` work across all of them.

RDS logs go to `/aws/rds/instance/bowline-<env>-postgres/postgresql` (exported by the database module) and VPC flow logs to `/bowline/<env>/vpc-flow-logs` (network module).

## Alarms

| Alarm                                | Source                                             | Fires when                                                    |
|--------------------------------------|----------------------------------------------------|---------------------------------------------------------------|
| `<env>-alb-5xx-rate`                 | `AWS/ApplicationELB` RequestCount, HTTPCode_Target_5XX_Count | target 5xx above 5% of requests in 2 of 3 minutes   |
| `<env>-<svc>-unhealthy-targets`      | `AWS/ApplicationELB` UnHealthyHostCount per target group | any api or web task fails `/healthz` for 3 minutes      |
| `<env>-rds-cpu`                      | `AWS/RDS` CPUUtilization                           | average above 80% for 15 minutes                              |
| `<env>-rds-free-storage`             | `AWS/RDS` FreeStorageSpace                         | under 5 GiB for 10 minutes                                    |
| `<env>-<svc>-running-below-desired`  | `ECS/ContainerInsights` DesiredTaskCount, RunningTaskCount | running below desired for 5 consecutive minutes       |
| `<env>-outbox-depth`                 | `Bowline/<env>` OutboxDepth (log-derived)          | more than 500 pending notifications for 15 minutes            |

Every alarm notifies the `bowline-<env>-alarms` SNS topic on both ALARM and OK. Set `alarm_email` to subscribe an address (the recipient must confirm the subscription). Add a pager or chat integration by subscribing to `sns_topic_arn` outside this module.

`docs/RUNBOOK.md` has the response procedure for each alarm.

## The outbox metric

`notify` exposes Prometheus metrics on `:9101`, but nothing scrapes Prometheus in this deployment. Instead the worker logs a JSON heartbeat on every poll cycle with an `outbox_depth` field (pending rows in `notifications`), and a metric filter on the notify log group turns that into `OutboxDepth` in the `Bowline/<env>` namespace. This is the cheapest way to get an application-level metric into CloudWatch without a sidecar; the same pattern extends to any other counter a service logs.

## Wiring note

This module both feeds the ecs module (log group names) and consumes its outputs (ALB and target group suffixes, service names). Terraform resolves dependencies per resource, so the mutual reference is fine as long as the environment root does not add `depends_on` between the two modules. The alarm toggles are literal booleans for the same reason: `count` must be known at plan time, and the ALB suffix is not.

## Inputs

| Name                               | Type         | Default                                     |
|------------------------------------|--------------|---------------------------------------------|
| `environment`                      | string       |                                             |
| `service_names`                    | list(string) | `["api","web","billing","analytics","notify"]` |
| `log_retention_days`               | number       | `30`                                        |
| `logs_kms_key_id`                  | string       | `null`                                      |
| `alarm_email`                      | string       | `""`                                        |
| `create_alb_alarms`                | bool         | `true`                                      |
| `alb_arn_suffix`                   | string       | `""`                                        |
| `target_group_arn_suffixes`        | map(string)  | `{}`                                        |
| `alb_5xx_rate_threshold_percent`   | number       | `5`                                         |
| `create_rds_alarms`                | bool         | `true`                                      |
| `db_instance_identifier`           | string       | `""`                                        |
| `rds_cpu_threshold_percent`        | number       | `80`                                        |
| `rds_free_storage_threshold_bytes` | number       | `5368709120`                                |
| `ecs_cluster_name`                 | string       | `""`                                        |
| `ecs_service_names`                | map(string)  | `{}`                                        |
| `custom_metric_namespace`          | string       | `null`                                      |
| `outbox_depth_threshold`           | number       | `500`                                       |
| `tags`                             | map(string)  | `{}`                                        |

## Outputs

`log_group_names`, `log_group_arns`, `sns_topic_arn`, `custom_metric_namespace`, `alarm_names`.
