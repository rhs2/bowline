variable "aws_region" {
  description = "Region every resource is created in."
  type        = string
  default     = "us-east-1"
}

variable "environment" {
  description = "Stage name. Fixed to staging for this root; it is a variable only so the TF_VAR_environment convention from .env.example works."
  type        = string
  default     = "staging"

  validation {
    condition     = var.environment == "staging"
    error_message = "This root manages the staging environment only."
  }
}

variable "image_tag" {
  description = "Image tag to deploy for every service (12-character commit SHA from the deploy workflow)."
  type        = string
}

variable "domain_name" {
  description = "Apex domain of the installation. Staging lives under it as staging.<domain_name>."
  type        = string
  default     = "bowline.example"
}

variable "public_hostname" {
  description = "Hostname of the staging application. Null derives staging.<domain_name>."
  type        = string
  default     = null
}

variable "mail_domain" {
  description = "SES sending domain for staging. Null derives staging.<domain_name> (distinct from production, which uses the apex)."
  type        = string
  default     = null
}

variable "mail_from" {
  description = "MAIL_FROM header. Null derives \"Bowline Staging <no-reply@<mail_domain>>\"."
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
  description = "VPC CIDR. Keep it distinct from production so the two could be peered."
  type        = string
  default     = "10.10.0.0/16"
}

variable "enable_execute_command" {
  description = "Allow ECS Exec into staging tasks."
  type        = bool
  default     = true
}

variable "tags" {
  description = "Extra default tags for every resource."
  type        = map(string)
  default     = {}
}
