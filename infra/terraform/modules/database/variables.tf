variable "environment" {
  description = "Deployment stage name (staging, prod)."
  type        = string
}

variable "subnet_ids" {
  description = "Isolated subnet ids for the DB subnet group (at least two, in different AZs)."
  type        = list(string)
}

variable "security_group_ids" {
  description = "Security groups attached to the instance (the network module's db group)."
  type        = list(string)
}

variable "instance_class" {
  description = "RDS instance class."
  type        = string
  default     = "db.t4g.medium"
}

variable "engine_version" {
  description = "PostgreSQL engine version. A major version prefix such as \"16\" lets RDS pick the current minor and keeps auto minor upgrades diff-free; the resolved version is in the engine_version_actual output."
  type        = string
  default     = "16"
}

variable "parameter_group_family" {
  description = "Parameter group family matching engine_version."
  type        = string
  default     = "postgres16"
}

variable "allocated_storage_gb" {
  description = "Initial gp3 storage in GiB."
  type        = number
  default     = 50
}

variable "max_allocated_storage_gb" {
  description = "Upper bound for storage autoscaling in GiB. Set to 0 to disable autoscaling."
  type        = number
  default     = 200
}

variable "multi_az" {
  description = "Synchronous standby in a second AZ with automatic failover."
  type        = bool
  default     = false
}

variable "backup_retention_days" {
  description = "Automated backup retention in days (7 to 35)."
  type        = number
  default     = 7

  validation {
    condition     = var.backup_retention_days >= 7 && var.backup_retention_days <= 35
    error_message = "backup_retention_days must be between 7 and 35."
  }
}

variable "deletion_protection" {
  description = "Refuse to delete the instance until this is switched off."
  type        = bool
  default     = true
}

variable "skip_final_snapshot" {
  description = "Skip the final snapshot on destroy. Keep false in production."
  type        = bool
  default     = false
}

variable "performance_insights_enabled" {
  description = "Enable Performance Insights."
  type        = bool
  default     = true
}

variable "performance_insights_retention_days" {
  description = "Performance Insights retention: 7 (free tier), 731, or a multiple of 31."
  type        = number
  default     = 7

  validation {
    condition     = var.performance_insights_retention_days == 7 || var.performance_insights_retention_days == 731 || var.performance_insights_retention_days % 31 == 0
    error_message = "performance_insights_retention_days must be 7, 731 or a multiple of 31."
  }
}

variable "monitoring_interval" {
  description = "Enhanced monitoring interval in seconds (0 disables it)."
  type        = number
  default     = 60

  validation {
    condition     = contains([0, 1, 5, 10, 15, 30, 60], var.monitoring_interval)
    error_message = "monitoring_interval must be one of 0, 1, 5, 10, 15, 30, 60."
  }
}

variable "log_min_duration_statement_ms" {
  description = "Statements slower than this many milliseconds are logged to the postgresql log (exported to CloudWatch). -1 disables slow query logging."
  type        = number
  default     = 500
}

variable "database_name" {
  description = "Name of the database created by RDS."
  type        = string
  default     = "bowline"
}

variable "master_username" {
  description = "Master (administrative) user. The application never connects as this user; the migrate task uses it once to create the three application roles."
  type        = string
  default     = "bowline_admin"
}

variable "kms_key_id" {
  description = "KMS key ARN for storage encryption. Null uses the AWS managed RDS key."
  type        = string
  default     = null
}

variable "apply_immediately" {
  description = "Apply modifications immediately instead of in the next maintenance window."
  type        = bool
  default     = false
}

variable "secret_recovery_window_days" {
  description = "Days before a deleted Secrets Manager secret is purged (0 for immediate, 7 to 30 otherwise)."
  type        = number
  default     = 7
}

variable "secrets_kms_key_id" {
  description = "KMS key for the Secrets Manager secrets. Null uses the AWS managed key."
  type        = string
  default     = null
}

variable "tags" {
  description = "Additional tags applied to every resource in this module."
  type        = map(string)
  default     = {}
}
