output "cluster_name" {
  description = "ECS cluster name (bowline-<environment>)."
  value       = aws_ecs_cluster.this.name
}

output "cluster_arn" {
  description = "ECS cluster ARN."
  value       = aws_ecs_cluster.this.arn
}

output "app_url" {
  description = "Public origin of the application."
  value       = local.public_origin
}

output "alb_dns_name" {
  description = "DNS name of the load balancer. Point public_hostname at it (alias record) if route53_zone_id was not given."
  value       = aws_lb.this.dns_name
}

output "alb_zone_id" {
  description = "Hosted zone id of the load balancer, for alias records."
  value       = aws_lb.this.zone_id
}

output "alb_arn" {
  description = "ARN of the load balancer."
  value       = aws_lb.this.arn
}

output "alb_arn_suffix" {
  description = "arn_suffix of the load balancer, the LoadBalancer dimension of CloudWatch metrics."
  value       = aws_lb.this.arn_suffix
}

output "target_group_arn_suffixes" {
  description = "arn_suffix of each target group keyed by service (api, web)."
  value       = { for k, tg in aws_lb_target_group.this : k => tg.arn_suffix }
}

output "service_names" {
  description = "ECS service names keyed by service."
  value = merge(
    { for k, s in aws_ecs_service.public : k => s.name },
    { for k, s in aws_ecs_service.internal : k => s.name },
  )
}

output "task_definition_arns" {
  description = "Task definition ARNs (with revision) keyed by service."
  value       = { for k, td in aws_ecs_task_definition.service : k => td.arn }
}

output "service_discovery_namespace" {
  description = "Cloud Map namespace name."
  value       = aws_service_discovery_private_dns_namespace.this.name
}

output "internal_urls" {
  description = "Internal base URLs the services use to reach each other."
  value = {
    api       = "http://api.${local.ns}:8080"
    billing   = "http://billing.${local.ns}:8081"
    analytics = "http://analytics.${local.ns}:8082"
  }
}

output "execution_role_arn" {
  description = "ARN of the shared task execution role."
  value       = aws_iam_role.execution.arn
}

output "task_role_arns" {
  description = "Task role ARNs keyed by service (including migrate)."
  value       = { for k, r in aws_iam_role.task : k => r.arn }
}

output "migrate_task_family" {
  description = "Task definition family of the migrate task."
  value       = aws_ecs_task_definition.migrate.family
}

output "migrate_task_definition_arn" {
  description = "Task definition ARN (with revision) of the migrate task."
  value       = aws_ecs_task_definition.migrate.arn
}

output "migrate_network_parameter_name" {
  description = "SSM parameter holding the network configuration JSON for `aws ecs run-task`."
  value       = aws_ssm_parameter.migrate_network.name
}
