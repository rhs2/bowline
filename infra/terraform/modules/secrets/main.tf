# Application secrets that are not tied to a managed service: the JWT signing
# key, the token the API sends to billing and analytics, and a JSON bundle
# combining them with any operator-supplied values.
#
# Rotation is `terraform apply -replace=<random resource>`; see docs/RUNBOOK.md.

locals {
  prefix = "bowline/${var.environment}"
}

# 32 random bytes as 64 hex characters, the same shape as `openssl rand -hex 32`.
resource "random_id" "jwt_secret" {
  byte_length = 32
}

resource "random_password" "internal_service_token" {
  length  = 48
  special = false
}

resource "aws_secretsmanager_secret" "jwt_secret" {
  name                    = "${local.prefix}/jwt-secret"
  description             = "HS256 signing key for access tokens (JWT_SECRET)"
  recovery_window_in_days = var.secret_recovery_window_days
  kms_key_id              = var.kms_key_id

  tags = var.tags
}

resource "aws_secretsmanager_secret_version" "jwt_secret" {
  secret_id     = aws_secretsmanager_secret.jwt_secret.id
  secret_string = random_id.jwt_secret.hex
}

resource "aws_secretsmanager_secret" "internal_service_token" {
  name                    = "${local.prefix}/internal-service-token"
  description             = "Shared secret sent as X-Internal-Token from api to billing and analytics (INTERNAL_SERVICE_TOKEN)"
  recovery_window_in_days = var.secret_recovery_window_days
  kms_key_id              = var.kms_key_id

  tags = var.tags
}

resource "aws_secretsmanager_secret_version" "internal_service_token" {
  secret_id     = aws_secretsmanager_secret.internal_service_token.id
  secret_string = random_password.internal_service_token.result
}

resource "aws_secretsmanager_secret" "app_bundle" {
  name                    = "${local.prefix}/app"
  description             = "JSON bundle of application secrets keyed by environment variable name"
  recovery_window_in_days = var.secret_recovery_window_days
  kms_key_id              = var.kms_key_id

  tags = var.tags
}

resource "aws_secretsmanager_secret_version" "app_bundle" {
  secret_id = aws_secretsmanager_secret.app_bundle.id

  secret_string = jsonencode(merge(
    var.extra_values,
    {
      JWT_SECRET             = random_id.jwt_secret.hex
      INTERNAL_SERVICE_TOKEN = random_password.internal_service_token.result
    },
  ))
}
