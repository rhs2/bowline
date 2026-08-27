# environments/staging

The staging root wires every module for a low-cost, single-AZ-tolerant copy of production. It is the environment `deploy.yml` targets when dispatched with `environment: staging`, and the one to use for rehearsing migrations, rotations and restores before doing them in production.

## What differs from production

| Concern            | Staging                                   | Production                              |
|--------------------|-------------------------------------------|-----------------------------------------|
| NAT                | one gateway                               | one per AZ                              |
| VPC endpoints      | S3 gateway only                           | plus ECR, Logs, Secrets Manager         |
| RDS                | `db.t4g.medium`, single AZ, 20 GiB, 7-day backups, no deletion protection, no final snapshot | `db.m7g.large`, Multi-AZ, 100 GiB, 35-day backups, deletion protection, final snapshot |
| Redis              | one `cache.t4g.micro`                     | two `cache.t4g.small`, failover, Multi-AZ |
| Tasks              | 0.25 vCPU / 512 MiB, one of each, api and web scale to 2 | 0.5 to 1 vCPU, two api and web, scale to 6 and 4 |
| Logs               | 14 days                                   | 90 days                                 |
| Secrets            | deleted immediately on destroy            | 30-day recovery window                  |
| S3                 | `force_destroy` on                        | off                                     |
| ECS Exec           | on                                        | off                                     |
| Hostnames          | `staging.<domain>` for app and mail       | `app.<domain>` and the apex for mail    |

## Usage

```
cd infra/terraform/environments/staging
cp terraform.tfvars.example terraform.tfvars   # fill in certificate_arn and image_tag
terraform init                                  # state backend from backend.tf
terraform plan -var image_tag=<sha>
terraform apply -var image_tag=<sha>
```

`image_tag` has no default on purpose: every apply pins the exact images running, and a rollback is an apply with the previous tag.

## Outputs

`app_url`, `alb_dns_name`, `rds_endpoint`, `redis_primary_endpoint`, `bucket_names`, `ecs_cluster_name`, `migrate_task_family`, `migrate_network_parameter`, `ses_dns_records`, `alarm_topic_arn`, `nat_gateway_public_ips`.
