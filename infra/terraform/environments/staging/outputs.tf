output "app_url" {
  description = "Public origin of the staging application."
  value       = module.ecs.app_url
}

output "alb_dns_name" {
  description = "Load balancer DNS name; alias public_hostname to it if DNS is managed elsewhere."
  value       = module.ecs.alb_dns_name
}

output "rds_endpoint" {
  description = "RDS writer endpoint."
  value       = module.database.endpoint
}

output "redis_primary_endpoint" {
  description = "Redis primary endpoint."
  value       = module.cache.primary_endpoint_address
}

output "bucket_names" {
  description = "S3 bucket names keyed by documents and pdfs."
  value       = module.storage.bucket_names
}

output "ecs_cluster_name" {
  description = "ECS cluster name."
  value       = module.ecs.cluster_name
}

output "migrate_task_family" {
  description = "Task definition family the deploy workflow runs for migrations."
  value       = module.ecs.migrate_task_family
}

output "migrate_network_parameter" {
  description = "SSM parameter with the run-task network configuration."
  value       = module.ecs.migrate_network_parameter_name
}

output "ses_dns_records" {
  description = "DNS records the mail domain needs (created automatically when route53_zone_id is set)."
  value       = module.mail.dns_records
}

output "alarm_topic_arn" {
  description = "SNS topic every alarm publishes to."
  value       = module.observability.sns_topic_arn
}

output "nat_gateway_public_ips" {
  description = "Egress IPs of the environment."
  value       = module.network.nat_gateway_public_ips
}
