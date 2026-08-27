variable "environment" {
  description = "Deployment stage name (staging, prod)."
  type        = string
}

variable "service_names" {
  description = "Application services that get a CloudWatch log group at /bowline/<env>/<service>. A migrate group is always added."
  type        = list(string)
  default     = ["api", "web", "billing", "analytics", "notify"]
}

variable "log_retention_days" {
  description = "Retention of the service log groups in days."
  type        = number
  default     = 30
}

variable "logs_kms_key_id" {
  description = "KMS key ARN for log group encryption. Null uses CloudWatch's default encryption."
  type        = string
  default     = null
}

variable "alarm_email" {
  description = "Email address subscribed to the alarm topic. Empty creates the topic with no subscription (the address must confirm the subscription by email)."
  type        = string
  default     = ""
}

variable "create_alb_alarms" {
  description = "Create the ALB 5xx rate and unhealthy target alarms. Must be a literal because it drives resource counts; alb_arn_suffix may be unknown until apply."
  type        = bool
  default     = true
}

variable "alb_arn_suffix" {
  description = "arn_suffix of the application load balancer (the ecs module's alb_arn_suffix output)."
  type        = string
  default     = ""
}

variable "target_group_arn_suffixes" {
  description = "Target group arn_suffix values keyed by service (api, web). One unhealthy-target alarm per entry."
  type        = map(string)
  default     = {}
}

variable "alb_5xx_rate_threshold_percent" {
  description = "Alarm when target 5xx responses exceed this percentage of requests over a minute."
  type        = number
  default     = 5
}

variable "create_rds_alarms" {
  description = "Create the RDS CPU and free storage alarms."
  type        = bool
  default     = true
}

variable "db_instance_identifier" {
  description = "RDS instance identifier (the database module's instance_identifier output)."
  type        = string
  default     = ""
}

variable "rds_cpu_threshold_percent" {
  description = "Alarm when average CPU stays above this percentage for 15 minutes."
  type        = number
  default     = 80
}

variable "rds_free_storage_threshold_bytes" {
  description = "Alarm when free storage drops under this many bytes (default 5 GiB)."
  type        = number
  default     = 5368709120
}

variable "ecs_cluster_name" {
  description = "ECS cluster name, bowline-<env>."
  type        = string
  default     = ""
}

variable "ecs_service_names" {
  description = "ECS service names keyed by service (the ecs module's service_names output). One running-below-desired alarm per entry."
  type        = map(string)
  default     = {}
}

variable "custom_metric_namespace" {
  description = "CloudWatch namespace for metrics derived from application logs. Null uses Bowline/<env>."
  type        = string
  default     = null
}

variable "outbox_depth_threshold" {
  description = "Alarm when the notifications outbox holds more than this many pending rows for 15 minutes."
  type        = number
  default     = 500
}

variable "tags" {
  description = "Additional tags applied to every resource in this module."
  type        = map(string)
  default     = {}
}
