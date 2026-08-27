output "vpc_id" {
  description = "Id of the VPC."
  value       = aws_vpc.this.id
}

output "vpc_cidr" {
  description = "CIDR block of the VPC."
  value       = aws_vpc.this.cidr_block
}

output "availability_zones" {
  description = "Availability zones in use, in subnet index order."
  value       = local.azs
}

output "public_subnet_ids" {
  description = "Public subnets (load balancer)."
  value       = aws_subnet.public[*].id
}

output "private_subnet_ids" {
  description = "Private subnets (ECS tasks)."
  value       = aws_subnet.private[*].id
}

output "isolated_subnet_ids" {
  description = "Isolated subnets (RDS, ElastiCache), no internet route."
  value       = aws_subnet.isolated[*].id
}

output "nat_gateway_public_ips" {
  description = "Public IPs of the NAT gateways. Useful for allow-listing at third parties."
  value       = aws_eip.nat[*].public_ip
}

output "alb_security_group_id" {
  description = "Security group for the application load balancer."
  value       = aws_security_group.alb.id
}

output "ecs_security_group_id" {
  description = "Security group for ECS tasks."
  value       = aws_security_group.ecs.id
}

output "db_security_group_id" {
  description = "Security group for the RDS instance."
  value       = aws_security_group.db.id
}

output "cache_security_group_id" {
  description = "Security group for the ElastiCache replication group."
  value       = aws_security_group.cache.id
}

output "endpoints_security_group_id" {
  description = "Security group attached to the interface VPC endpoints."
  value       = aws_security_group.endpoints.id
}

output "flow_log_group_name" {
  description = "CloudWatch log group receiving VPC flow logs, or null when disabled."
  value       = var.enable_flow_logs ? aws_cloudwatch_log_group.flow_logs[0].name : null
}
