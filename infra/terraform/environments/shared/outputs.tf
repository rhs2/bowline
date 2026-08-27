output "deploy_role_arn" {
  description = "Store this as the AWS_DEPLOY_ROLE_ARN repository secret in GitHub."
  value       = aws_iam_role.deploy.arn
}

output "oidc_provider_arn" {
  description = "ARN of the GitHub OIDC provider in use."
  value       = local.oidc_provider_arn
}

output "registry_url" {
  description = "ECR registry hostname."
  value       = module.ecr.registry_url
}

output "repository_urls" {
  description = "ECR repository URLs keyed by service."
  value       = module.ecr.repository_urls
}
