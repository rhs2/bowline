# network

VPC, subnets, NAT, VPC endpoints, security groups and flow logs for one Bowline environment.

## Layout

The VPC CIDR (default `10.0.0.0/16`) is carved into /20 blocks so the plan is the same with two or three availability zones:

| Tier     | Block index | Hosts                          | Internet                     |
|----------|-------------|--------------------------------|------------------------------|
| public   | 0 to 3      | application load balancer      | inbound via internet gateway |
| private  | 4 to 7      | ECS Fargate tasks              | outbound via NAT only        |
| isolated | 8 to 11     | RDS PostgreSQL, ElastiCache    | none                         |

`single_nat_gateway = true` (staging) shares one NAT gateway across all private subnets. Production sets it to `false` for one NAT per AZ, so losing an AZ does not take away egress from the others.

## Endpoints

- S3 gateway endpoint (free): attached to the private and isolated route tables. Presigned uploads from browsers do not use it; the API, billing and ECR image layers do.
- Interface endpoints (`enable_interface_endpoints`): ECR api, ECR dkr, CloudWatch Logs, Secrets Manager. With them enabled, image pulls, log shipping and secret reads stay inside the VPC. Staging turns them off and pays for NAT data processing instead, which is cheaper at low volume.

## Security groups

| Group       | Inbound                                            | Outbound                                                        |
|-------------|----------------------------------------------------|-----------------------------------------------------------------|
| `alb`       | 80 and 443 from anywhere                           | 8080 (api) and 3000 (web) to `ecs`                              |
| `ecs`       | 8080, 3000 from `alb`; 8080, 8081, 8082 from `ecs` | 5432 to `db`, 6379 to `cache`, 8080/8081/8082 to `ecs`, 443 and 587 to anywhere |
| `db`        | 5432 from `ecs`                                    | none                                                            |
| `cache`     | 6379 from `ecs`                                    | none                                                            |
| `endpoints` | 443 from `ecs`                                     | none                                                            |

Every rule is its own resource with a description, which keeps the console readable and lets groups reference each other without a cycle. The `ecs` group is shared by all five services; per-service groups would add little because Cloud Map addresses are only resolvable inside the VPC and the internal services check `X-Internal-Token` on every request.

## Flow logs

`enable_flow_logs` writes all traffic to `/bowline/<environment>/vpc-flow-logs` through a dedicated IAM role. Retention defaults to 30 days.

## Inputs

| Name                         | Type        | Default         | Description                                                         |
|------------------------------|-------------|-----------------|---------------------------------------------------------------------|
| `environment`                | string      |                 | Stage name used in resource names                                   |
| `vpc_cidr`                   | string      | `10.0.0.0/16`   | VPC CIDR, must be /20 or larger                                     |
| `az_count`                   | number      | `2`             | Availability zones (2 or 3)                                         |
| `single_nat_gateway`         | bool        | `true`          | One NAT for all private subnets, or one per AZ                      |
| `enable_interface_endpoints` | bool        | `true`          | ECR, Logs and Secrets Manager interface endpoints                   |
| `enable_flow_logs`           | bool        | `true`          | VPC flow logs to CloudWatch                                         |
| `flow_log_retention_days`    | number      | `30`            | Flow log retention                                                  |
| `tags`                       | map(string) | `{}`            | Extra tags                                                          |

## Outputs

`vpc_id`, `vpc_cidr`, `availability_zones`, `public_subnet_ids`, `private_subnet_ids`, `isolated_subnet_ids`, `nat_gateway_public_ips`, `alb_security_group_id`, `ecs_security_group_id`, `db_security_group_id`, `cache_security_group_id`, `endpoints_security_group_id`, `flow_log_group_name`.
