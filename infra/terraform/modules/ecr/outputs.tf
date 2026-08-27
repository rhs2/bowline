output "registry_id" {
  description = "AWS account id that owns the registry."
  value       = data.aws_caller_identity.current.account_id
}

output "registry_url" {
  description = "Registry hostname, <account id>.dkr.ecr.<region>.amazonaws.com."
  value       = local.registry_url
}

output "repository_names" {
  description = "Repository names keyed by service (bowline/api, ...)."
  value       = { for k, r in aws_ecr_repository.this : k => r.name }
}

output "repository_urls" {
  description = "Full repository URLs keyed by service."
  value       = { for k, r in aws_ecr_repository.this : k => r.repository_url }
}

output "repository_arns" {
  description = "Repository ARNs keyed by service."
  value       = { for k, r in aws_ecr_repository.this : k => r.arn }
}
