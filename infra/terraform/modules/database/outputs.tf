output "endpoint" {
  description = "Hostname of the RDS instance (writer)."
  value       = aws_db_instance.this.address
}

output "port" {
  description = "Port of the RDS instance."
  value       = aws_db_instance.this.port
}

output "database_name" {
  description = "Name of the database."
  value       = var.database_name
}

output "instance_identifier" {
  description = "RDS instance identifier, used by CloudWatch alarms and snapshot commands."
  value       = aws_db_instance.this.identifier
}

output "instance_arn" {
  description = "ARN of the RDS instance."
  value       = aws_db_instance.this.arn
}

output "engine_version_actual" {
  description = "Engine version actually running."
  value       = aws_db_instance.this.engine_version_actual
}

output "master_username" {
  description = "Master user name."
  value       = var.master_username
}

output "master_secret_arn" {
  description = "Secrets Manager ARN of the master credentials (JSON: host, port, dbname, username, password, url, jdbc_url)."
  value       = aws_secretsmanager_secret.master.arn
}

output "master_secret_name" {
  description = "Secrets Manager name of the master credentials."
  value       = aws_secretsmanager_secret.master.name
}

output "role_names" {
  description = "PostgreSQL role names keyed by app, ro, notify."
  value       = local.roles
}

output "role_secret_arns" {
  description = "Secrets Manager ARNs of the application role credentials keyed by app, ro, notify. Same JSON layout as the master secret."
  value       = { for k, s in aws_secretsmanager_secret.role : k => s.arn }
}

output "role_secret_names" {
  description = "Secrets Manager names of the application role credentials keyed by app, ro, notify."
  value       = { for k, s in aws_secretsmanager_secret.role : k => s.name }
}

output "parameter_group_name" {
  description = "Name of the parameter group."
  value       = aws_db_parameter_group.this.name
}
