output "jwt_secret_arn" {
  description = "Secrets Manager ARN of JWT_SECRET (plain string value)."
  value       = aws_secretsmanager_secret.jwt_secret.arn
}

output "jwt_secret_name" {
  description = "Secrets Manager name of JWT_SECRET."
  value       = aws_secretsmanager_secret.jwt_secret.name
}

output "internal_service_token_arn" {
  description = "Secrets Manager ARN of INTERNAL_SERVICE_TOKEN (plain string value)."
  value       = aws_secretsmanager_secret.internal_service_token.arn
}

output "internal_service_token_name" {
  description = "Secrets Manager name of INTERNAL_SERVICE_TOKEN."
  value       = aws_secretsmanager_secret.internal_service_token.name
}

output "app_bundle_arn" {
  description = "Secrets Manager ARN of the JSON bundle (keys: JWT_SECRET, INTERNAL_SERVICE_TOKEN, plus extra_values)."
  value       = aws_secretsmanager_secret.app_bundle.arn
}

output "app_bundle_name" {
  description = "Secrets Manager name of the JSON bundle."
  value       = aws_secretsmanager_secret.app_bundle.name
}

output "all_secret_arns" {
  description = "Every secret ARN this module manages, for execution role policies."
  value = [
    aws_secretsmanager_secret.jwt_secret.arn,
    aws_secretsmanager_secret.internal_service_token.arn,
    aws_secretsmanager_secret.app_bundle.arn,
  ]
}
