variable "aws_region" {
  description = "Region every resource is created in."
  type        = string
  default     = "us-east-1"
}

variable "environment" {
  description = "Stage name. Fixed to prod for this root; it is a variable only so the TF_VAR_environment convention from .env.example works."
  type        = string
  default     = "prod"

  validation {
    condition     = var.environment == "prod"
    error_message = "This root manages the prod environment only."
  }
}

variable "image_tag" {
  description = "Image tag to deploy for every service (12-character commit SHA from the deploy workflow)."
  type        = string
}

variable "domain_name" {
  description = "Apex domain of the installation. Mail is sent from it; the application lives at app.<domain_name> unless public_hostname says otherwise."
  type        = string
  default     = "bowline.example"
}

variable "public_hostname" {
  description = "Hostname of the application. Null derives app.<domain_name>."
  type        = string
  default     = null
}

variable "mail_domain" {
  description = "SES sending domain. Null uses domain_name."
  type        = string
  default     = null
}

variable "mail_from" {
  description = "MAIL_FROM header. Null derives \"Bowline <no-reply@<mail_domain>>\"."
  type        = string
  default     = null
}

variable "certificate_arn" {
  description = "ACM certificate ARN covering public_hostname, in aws_region."
  type        = string
}

variable "route53_zone_id" {
  description = "Hosted zone for the application alias record and the SES DNS records. Null skips DNS management."
  type        = string
  default     = null
}

variable "dmarc_record" {
  description = "DMARC TXT value for the mail domain, or null."
  type        = string
  default     = null
}

variable "alarm_email" {
  description = "Address subscribed to the alarm topic. Empty for none."
  type        = string
  default     = ""
}

variable "vpc_cidr" {
  description = "VPC CIDR."
  type        = string
  default     = "10.20.0.0/16"
}

variable "az_count" {
  description = "Availability zones (2 or 3)."
  type        = number
  default     = 3
}

variable "db_instance_class" {
  description = "RDS instance class."
  type        = string
  default     = "db.m7g.large"
}

variable "cache_node_type" {
  description = "ElastiCache node type."
  type        = string
  default     = "cache.t4g.small"
}

variable "services" {
  description = "Per-service Fargate sizing; see modules/ecs. Defaults are production sizes."
  type = map(object({
    cpu                = number
    memory             = number
    desired_count      = number
    min_count          = optional(number)
    max_count          = optional(number)
    cpu_target_percent = optional(number, 60)
  }))

  default = {
    api       = { cpu = 1024, memory = 2048, desired_count = 2, min_count = 2, max_count = 6 }
    web       = { cpu = 512, memory = 1024, desired_count = 2, min_count = 2, max_count = 4 }
    billing   = { cpu = 1024, memory = 2048, desired_count = 1 }
    analytics = { cpu = 1024, memory = 2048, desired_count = 1 }
    notify    = { cpu = 256, memory = 512, desired_count = 1 }
  }
}

variable "enable_execute_command" {
  description = "Allow ECS Exec into production tasks. Off unless an incident needs it."
  type        = bool
  default     = false
}

variable "tags" {
  description = "Extra default tags for every resource."
  type        = map(string)
  default     = {}
}
