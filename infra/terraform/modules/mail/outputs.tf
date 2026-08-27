output "domain_name" {
  description = "Verified sending domain."
  value       = var.domain_name
}

output "identity_arn" {
  description = "ARN of the SES domain identity (for IAM send policies)."
  value       = aws_sesv2_email_identity.domain.arn
}

output "configuration_set_name" {
  description = "Name of the configuration set."
  value       = aws_sesv2_configuration_set.this.configuration_set_name
}

output "configuration_set_arn" {
  description = "ARN of the configuration set (for IAM send policies)."
  value       = aws_sesv2_configuration_set.this.arn
}

output "dkim_tokens" {
  description = "The three DKIM tokens SES issued for the domain."
  value       = local.dkim_tokens
}

output "dns_records" {
  description = "Every DNS record the domain needs (DKIM CNAMEs, MAIL FROM MX and SPF, DMARC). Created automatically when route53_zone_id is set; otherwise create them at your DNS provider."
  value       = concat(local.dkim_records, local.mail_from_records, local.dmarc_records)
}

output "mail_from_domain" {
  description = "Custom MAIL FROM domain, or null."
  value       = local.mail_from_domain
}

output "smtp_host" {
  description = "SES SMTP endpoint for the region."
  value       = local.smtp_host
}

output "smtp_port" {
  description = "SES SMTP port (STARTTLS)."
  value       = 587
}

output "smtp_user_name" {
  description = "IAM user whose access key backs the SMTP credential."
  value       = aws_iam_user.smtp.name
}

output "smtp_secret_arn" {
  description = "Secrets Manager ARN of the SMTP credential (JSON keys: host, port, starttls, username, password)."
  value       = aws_secretsmanager_secret.smtp.arn
}

output "smtp_secret_name" {
  description = "Secrets Manager name of the SMTP credential."
  value       = aws_secretsmanager_secret.smtp.name
}
