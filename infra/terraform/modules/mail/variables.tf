variable "environment" {
  description = "Deployment stage name (staging, prod)."
  type        = string
}

variable "domain_name" {
  description = "Domain to verify as an SES sending identity. An SES identity is account-wide per region, so environments in the same account need distinct domains (bowline.example for prod, staging.bowline.example for staging)."
  type        = string
}

variable "mail_from_subdomain" {
  description = "Subdomain used as the custom MAIL FROM domain (envelope sender), giving SPF alignment for DMARC. Null keeps the amazonses.com default."
  type        = string
  default     = "mail"
}

variable "route53_zone_id" {
  description = "Hosted zone id in which to create the DKIM, MAIL FROM and DMARC records. Null skips DNS; the records are then available in the dns_records output for manual creation."
  type        = string
  default     = null
}

variable "dmarc_record" {
  description = "Value of the _dmarc TXT record, for example \"v=DMARC1; p=quarantine; rua=mailto:dmarc@bowline.example\". Null creates no DMARC record."
  type        = string
  default     = null
}

variable "secret_recovery_window_days" {
  description = "Days before a deleted Secrets Manager secret is purged."
  type        = number
  default     = 7
}

variable "secrets_kms_key_id" {
  description = "KMS key for the SMTP credential secret. Null uses the AWS managed key."
  type        = string
  default     = null
}

variable "tags" {
  description = "Additional tags applied to every resource in this module."
  type        = map(string)
  default     = {}
}
