# environments/prod

The production root. `deploy.yml` applies it on every push to `main` (behind the protected `prod` GitHub environment) with `-var image_tag=<sha>`, then runs the `bowline-migrate-prod` task and fails the deploy if it exits non-zero.

## Shape

- Three availability zones, one NAT gateway per AZ, interface endpoints for ECR, CloudWatch Logs and Secrets Manager.
- RDS `db.m7g.large`, Multi-AZ, 100 GiB gp3 growing to 500, 35-day automated backups, deletion protection, final snapshot on destroy, Performance Insights, enhanced monitoring.
- Redis: two `cache.t4g.small` nodes with automatic failover across AZs, three days of snapshots.
- Tasks: api 1 vCPU / 2 GiB scaling 2 to 6, web 0.5 vCPU / 1 GiB scaling 2 to 4, billing and analytics 1 vCPU / 2 GiB, notify 0.25 vCPU / 512 MiB.
- Logs kept 90 days; every secret has a 30-day recovery window; buckets cannot be force-destroyed; the ALB has deletion protection; ECS Exec is off.
- Hostnames: application at `app.<domain_name>`, mail from the apex domain.

## Usage

```
cd infra/terraform/environments/prod
terraform init
terraform plan  -var image_tag=<sha>
terraform apply -var image_tag=<sha>
```

Manual applies should be rare: the deploy workflow is the normal path, and `docs/RUNBOOK.md` covers rollback (apply with the previous tag), migrations, restores and rotations.

## Changing sizes

Override `services`, `db_instance_class`, `cache_node_type` or `az_count` in `terraform.tfvars`. Changing the RDS instance class is applied in the maintenance window (`apply_immediately = false`); set it to true in the module call for an immediate change and expect a failover-length interruption (about a minute on Multi-AZ).

## Outputs

`app_url`, `alb_dns_name`, `rds_endpoint`, `redis_primary_endpoint`, `bucket_names`, `ecs_cluster_name`, `migrate_task_family`, `migrate_network_parameter`, `ses_dns_records`, `alarm_topic_arn`, `nat_gateway_public_ips`.
