variable "environment" {
  description = "Deployment stage name (staging, prod)."
  type        = string
}

variable "subnet_ids" {
  description = "Isolated subnet ids for the cache subnet group."
  type        = list(string)
}

variable "security_group_ids" {
  description = "Security groups attached to the replication group (the network module's cache group)."
  type        = list(string)
}

variable "node_type" {
  description = "ElastiCache node type."
  type        = string
  default     = "cache.t4g.micro"
}

variable "engine_version" {
  description = "Redis engine version (7.x)."
  type        = string
  default     = "7.1"
}

variable "parameter_group_family" {
  description = "Parameter group family matching engine_version."
  type        = string
  default     = "redis7"
}

variable "num_cache_clusters" {
  description = "Number of nodes (one primary plus replicas). Use 1 for staging, 2 or more for production."
  type        = number
  default     = 1

  validation {
    condition     = var.num_cache_clusters >= 1 && var.num_cache_clusters <= 6
    error_message = "num_cache_clusters must be between 1 and 6."
  }
}

variable "automatic_failover_enabled" {
  description = "Promote a replica automatically when the primary fails. Requires num_cache_clusters >= 2."
  type        = bool
  default     = false
}

variable "multi_az_enabled" {
  description = "Place replicas in other availability zones. Requires automatic failover."
  type        = bool
  default     = false
}

variable "snapshot_retention_limit" {
  description = "Days to keep automatic snapshots (0 disables them). The cache holds only ephemeral state so 1 is plenty."
  type        = number
  default     = 1
}

variable "kms_key_id" {
  description = "KMS key ARN for encryption at rest. Null uses the AWS managed key."
  type        = string
  default     = null
}

variable "apply_immediately" {
  description = "Apply modifications immediately instead of in the next maintenance window."
  type        = bool
  default     = false
}

variable "secret_recovery_window_days" {
  description = "Days before a deleted Secrets Manager secret is purged."
  type        = number
  default     = 7
}

variable "secrets_kms_key_id" {
  description = "KMS key for the Secrets Manager secret. Null uses the AWS managed key."
  type        = string
  default     = null
}

variable "tags" {
  description = "Additional tags applied to every resource in this module."
  type        = map(string)
  default     = {}
}
