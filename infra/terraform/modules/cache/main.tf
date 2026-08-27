# ElastiCache Redis 7 replication group. Holds the principal cache (60 s TTL),
# rate limiter counters and login lockout counters. Nothing durable lives here,
# so a cache loss costs a few seconds of extra database load and nothing else.

locals {
  name = "bowline-${var.environment}"
}

# Redis AUTH tokens must be 16 to 128 printable ASCII characters without '@',
# '"' or '/'. Alphanumeric only keeps the token safe inside a URL as well.
resource "random_password" "auth_token" {
  length  = 64
  special = false
}

resource "aws_elasticache_subnet_group" "this" {
  name        = "${local.name}-cache"
  description = "Isolated subnets for the ${var.environment} cache"
  subnet_ids  = var.subnet_ids

  tags = var.tags
}

resource "aws_elasticache_parameter_group" "this" {
  name        = "${local.name}-redis7"
  family      = var.parameter_group_family
  description = "Bowline ${var.environment} Redis 7"

  # Every key Bowline writes carries a TTL; evicting only keys with a TTL keeps
  # lockout counters from disappearing under memory pressure ahead of cache
  # entries that are about to expire anyway.
  parameter {
    name  = "maxmemory-policy"
    value = "volatile-lru"
  }

  tags = var.tags

  lifecycle {
    create_before_destroy = true
  }
}

resource "aws_elasticache_replication_group" "this" {
  replication_group_id = "${local.name}-redis"
  description          = "Bowline ${var.environment} principal cache and rate limiter"

  engine         = "redis"
  engine_version = var.engine_version
  node_type      = var.node_type
  port           = 6379

  num_cache_clusters         = var.num_cache_clusters
  automatic_failover_enabled = var.automatic_failover_enabled
  multi_az_enabled           = var.multi_az_enabled

  subnet_group_name    = aws_elasticache_subnet_group.this.name
  security_group_ids   = var.security_group_ids
  parameter_group_name = aws_elasticache_parameter_group.this.name

  at_rest_encryption_enabled = true
  kms_key_id                 = var.kms_key_id
  transit_encryption_enabled = true
  auth_token                 = random_password.auth_token.result
  auth_token_update_strategy = "ROTATE"

  snapshot_retention_limit = var.snapshot_retention_limit
  snapshot_window          = "02:00-03:00"
  maintenance_window       = "sun:03:30-sun:04:30"

  auto_minor_version_upgrade = true
  apply_immediately          = var.apply_immediately

  tags = merge(var.tags, { Name = "${local.name}-redis" })

  lifecycle {
    precondition {
      condition     = !var.automatic_failover_enabled || var.num_cache_clusters >= 2
      error_message = "automatic_failover_enabled requires num_cache_clusters >= 2."
    }

    precondition {
      condition     = !var.multi_az_enabled || var.automatic_failover_enabled
      error_message = "multi_az_enabled requires automatic_failover_enabled."
    }
  }
}

resource "aws_secretsmanager_secret" "this" {
  name                    = "bowline/${var.environment}/redis"
  description             = "Redis AUTH token and TLS connection URL for bowline-${var.environment}"
  recovery_window_in_days = var.secret_recovery_window_days
  kms_key_id              = var.secrets_kms_key_id

  tags = var.tags
}

resource "aws_secretsmanager_secret_version" "this" {
  secret_id = aws_secretsmanager_secret.this.id

  secret_string = jsonencode({
    auth_token       = random_password.auth_token.result
    primary_endpoint = aws_elasticache_replication_group.this.primary_endpoint_address
    reader_endpoint  = aws_elasticache_replication_group.this.reader_endpoint_address
    port             = 6379
    url              = "rediss://:${random_password.auth_token.result}@${aws_elasticache_replication_group.this.primary_endpoint_address}:6379/0"
  })
}
