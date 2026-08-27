# ---- Identity and placement ------------------------------------------------

variable "environment" {
  description = "Deployment stage name (staging, prod). The cluster is named bowline-<environment>."
  type        = string
}

variable "vpc_id" {
  description = "VPC id (network module)."
  type        = string
}

variable "public_subnet_ids" {
  description = "Public subnets for the load balancer."
  type        = list(string)
}

variable "private_subnet_ids" {
  description = "Private subnets for the tasks."
  type        = list(string)
}

variable "alb_security_group_id" {
  description = "Security group for the load balancer."
  type        = string
}

variable "ecs_security_group_id" {
  description = "Security group for the tasks."
  type        = string
}

# ---- Public entry point ----------------------------------------------------

variable "public_hostname" {
  description = "Hostname users open in the browser, for example app.bowline.example. The web app and the API share it: /api/*, /docs and /api-docs/* route to api, everything else to web."
  type        = string
}

variable "certificate_arn" {
  description = "ACM certificate ARN for public_hostname, in the same region as the load balancer."
  type        = string
}

variable "ssl_policy" {
  description = "TLS policy of the HTTPS listener."
  type        = string
  default     = "ELBSecurityPolicy-TLS13-1-2-2021-06"
}

variable "route53_zone_id" {
  description = "Hosted zone in which to create the alias record for public_hostname. Null skips the record."
  type        = string
  default     = null
}

variable "alb_deletion_protection" {
  description = "Protect the load balancer from deletion."
  type        = bool
  default     = true
}

variable "alb_access_logs_bucket" {
  description = "S3 bucket for ALB access logs (must already allow the regional ELB account to write). Null disables access logs."
  type        = string
  default     = null
}

# ---- Images ----------------------------------------------------------------

variable "ecr_registry" {
  description = "Registry hostname, <account id>.dkr.ecr.<region>.amazonaws.com."
  type        = string
}

variable "image_name_prefix" {
  description = "Repository namespace, so images are <registry>/<prefix>/<service>:<tag>."
  type        = string
  default     = "bowline"
}

variable "image_tag" {
  description = "Image tag deployed for every service. The deploy workflow passes the 12-character commit SHA; rolling back means applying with the previous tag."
  type        = string
}

# ---- Sizing ----------------------------------------------------------------

variable "services" {
  description = "Per-service Fargate size and counts. cpu and memory are Fargate units (256/512, 512/1024, 1024/2048, ...). min_count and max_count enable CPU target-tracking autoscaling for api and web; they are ignored for the other services."
  type = map(object({
    cpu                = number
    memory             = number
    desired_count      = number
    min_count          = optional(number)
    max_count          = optional(number)
    cpu_target_percent = optional(number, 60)
  }))

  default = {
    api       = { cpu = 512, memory = 1024, desired_count = 2, min_count = 2, max_count = 6 }
    web       = { cpu = 512, memory = 1024, desired_count = 2, min_count = 2, max_count = 4 }
    billing   = { cpu = 1024, memory = 2048, desired_count = 1 }
    analytics = { cpu = 512, memory = 1024, desired_count = 1 }
    notify    = { cpu = 256, memory = 512, desired_count = 1 }
  }

  validation {
    condition     = toset(keys(var.services)) == toset(["api", "web", "billing", "analytics", "notify"])
    error_message = "services must have exactly the keys api, web, billing, analytics and notify."
  }
}

variable "migrate_task_family" {
  description = "Task definition family of the one-off migration task. Null derives bowline-migrate-<environment>, which is what the deploy workflow resolves (FAMILY=\"bowline-migrate-$ENVIRONMENT\"), so leave it null unless the workflow changes too. The family carries the environment because families are account-wide: a single shared family would resolve to whichever environment was applied most recently, and a staging deploy could then migrate production."
  type        = string
  default     = null
}

variable "migrate_cpu" {
  description = "Fargate CPU units for the migrate task."
  type        = number
  default     = 512
}

variable "migrate_memory" {
  description = "Fargate memory (MiB) for the migrate task."
  type        = number
  default     = 1024
}

variable "enable_execute_command" {
  description = "Allow `aws ecs execute-command` into running tasks (grants ssmmessages to every task role). Useful in staging, off by default in production."
  type        = bool
  default     = false
}

variable "service_discovery_namespace" {
  description = "Cloud Map private DNS namespace. api, billing and analytics register as <service>.<namespace>."
  type        = string
  default     = "bowline.local"
}

variable "log_group_names" {
  description = "CloudWatch log group per container, keyed by api, web, billing, analytics, notify and migrate (observability module output)."
  type        = map(string)
}

# ---- Secrets (ARNs) --------------------------------------------------------

variable "db_master_secret_arn" {
  description = "Secret with the RDS master credentials (database module). Only the migrate task reads it."
  type        = string
}

variable "db_role_secret_arns" {
  description = "Secrets for the application roles keyed by app, ro, notify (database module)."
  type        = map(string)
}

variable "redis_secret_arn" {
  description = "Secret with the Redis URL (cache module)."
  type        = string
}

variable "jwt_secret_arn" {
  description = "Secret holding JWT_SECRET (secrets module)."
  type        = string
}

variable "internal_service_token_secret_arn" {
  description = "Secret holding INTERNAL_SERVICE_TOKEN (secrets module)."
  type        = string
}

variable "smtp_secret_arn" {
  description = "Secret with the SES SMTP credentials (mail module)."
  type        = string
}

variable "secrets_kms_key_arn" {
  description = "Customer managed KMS key the secrets are encrypted with, if any. The execution role gets kms:Decrypt on it. Null when the AWS managed key is used."
  type        = string
  default     = null
}

# ---- Storage and mail ------------------------------------------------------

variable "s3_bucket_names" {
  description = "Bucket names keyed by documents and pdfs (storage module)."
  type        = map(string)
}

variable "s3_bucket_arns" {
  description = "Bucket ARNs keyed by documents and pdfs (storage module)."
  type        = map(string)
}

variable "s3_kms_key_arn" {
  description = "KMS key the buckets encrypt with; task roles that presign uploads need it."
  type        = string
}

variable "s3_region" {
  description = "Region of the buckets (S3_REGION). Null uses the current region."
  type        = string
  default     = null
}

variable "ses_identity_arn" {
  description = "SES domain identity ARN, granted to the notify task role (mail module)."
  type        = string
}

variable "ses_configuration_set_arn" {
  description = "SES configuration set ARN, granted to the notify task role (mail module)."
  type        = string
}

variable "smtp_host" {
  description = "SES SMTP endpoint (mail module smtp_host output)."
  type        = string
}

variable "smtp_port" {
  description = "SES SMTP port."
  type        = number
  default     = 587
}

variable "mail_from" {
  description = "MAIL_FROM header value, for example \"Bowline <no-reply@bowline.example>\". The address must be at the verified SES domain."
  type        = string
}

# ---- Application configuration (plain values from .env.example) ------------

variable "app_name" {
  description = "NEXT_PUBLIC_APP_NAME."
  type        = string
  default     = "Bowline"
}

variable "database_max_connections" {
  description = "DATABASE_MAX_CONNECTIONS for the API pool."
  type        = number
  default     = 20
}

variable "jwt_issuer" {
  description = "JWT_ISSUER."
  type        = string
  default     = "bowline"
}

variable "access_token_ttl_seconds" {
  description = "ACCESS_TOKEN_TTL_SECONDS."
  type        = number
  default     = 900
}

variable "refresh_token_ttl_seconds" {
  description = "REFRESH_TOKEN_TTL_SECONDS."
  type        = number
  default     = 2592000
}

variable "login_max_failures" {
  description = "LOGIN_MAX_FAILURES."
  type        = number
  default     = 5
}

variable "login_lockout_seconds" {
  description = "LOGIN_LOCKOUT_SECONDS."
  type        = number
  default     = 900
}

variable "rate_limit_per_minute" {
  description = "RATE_LIMIT_PER_MINUTE."
  type        = number
  default     = 300
}

variable "invoice_approval_threshold" {
  description = "INVOICE_APPROVAL_THRESHOLD."
  type        = number
  default     = 50000
}

variable "rust_log" {
  description = "RUST_LOG filter for api and migrate."
  type        = string
  default     = "info,bowline_api=info,sqlx=warn"
}

variable "presign_ttl_seconds" {
  description = "PRESIGN_TTL_SECONDS."
  type        = number
  default     = 900
}

variable "notify_poll_interval_ms" {
  description = "NOTIFY_POLL_INTERVAL_MS."
  type        = number
  default     = 2000
}

variable "notify_batch_size" {
  description = "NOTIFY_BATCH_SIZE."
  type        = number
  default     = 50
}

variable "notify_max_attempts" {
  description = "NOTIFY_MAX_ATTEMPTS."
  type        = number
  default     = 8
}

variable "billing_company_name" {
  description = "BILLING_COMPANY_NAME printed on invoices."
  type        = string
  default     = "Bowline Logistics"
}

variable "billing_company_address" {
  description = "BILLING_COMPANY_ADDRESS printed on invoices."
  type        = string
  default     = "1 Harbour Way, Port City"
}

variable "analytics_model_path" {
  description = "ANALYTICS_MODEL_PATH inside the analytics image."
  type        = string
  default     = "/app/models/delay_risk.joblib"
}

variable "extra_environment" {
  description = "Additional plain environment variables per service, keyed by service name then variable name. Merged over the module's defaults."
  type        = map(map(string))
  default     = {}
}

variable "tags" {
  description = "Additional tags applied to every resource in this module."
  type        = map(string)
  default     = {}
}
