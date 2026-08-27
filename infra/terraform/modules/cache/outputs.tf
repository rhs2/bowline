output "replication_group_id" {
  description = "Id of the replication group."
  value       = aws_elasticache_replication_group.this.id
}

output "primary_endpoint_address" {
  description = "Primary (writer) endpoint hostname."
  value       = aws_elasticache_replication_group.this.primary_endpoint_address
}

output "reader_endpoint_address" {
  description = "Reader endpoint hostname."
  value       = aws_elasticache_replication_group.this.reader_endpoint_address
}

output "port" {
  description = "Redis port."
  value       = 6379
}

output "secret_arn" {
  description = "Secrets Manager ARN of the AUTH token secret (JSON keys: auth_token, primary_endpoint, reader_endpoint, port, url)."
  value       = aws_secretsmanager_secret.this.arn
}

output "secret_name" {
  description = "Secrets Manager name of the AUTH token secret."
  value       = aws_secretsmanager_secret.this.name
}
