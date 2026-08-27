# RDS PostgreSQL 16 for one environment, plus the credentials in Secrets Manager.
#
# Four secrets are written:
#   bowline/<env>/db/master   the RDS master user, used only by the migrate task
#   bowline/<env>/db/app      bowline_app    (api, read/write)
#   bowline/<env>/db/ro       bowline_ro     (billing, analytics, read-only)
#   bowline/<env>/db/notify   bowline_notify (notify, outbox table only)
#
# RDS only creates the master user. The three application roles are created by
# the migrate task (see README.md), which receives the generated passwords
# through its task definition. Terraform therefore never talks to PostgreSQL.

locals {
  name = "bowline-${var.environment}"

  roles = {
    app    = "bowline_app"
    ro     = "bowline_ro"
    notify = "bowline_notify"
  }

  host = aws_db_instance.this.address
  port = aws_db_instance.this.port

  # sslmode=require pairs with rds.force_ssl=1 in the parameter group.
  jdbc_url = "jdbc:postgresql://${local.host}:${local.port}/${var.database_name}?sslmode=require"
}

# ---- Passwords -------------------------------------------------------------

# RDS forbids '/', '@', '"' and spaces in the master password. The remaining
# specials are URL-encoded when the connection URL is built.
resource "random_password" "master" {
  length           = 32
  special          = true
  override_special = "!#$%^&*()-_=+"
  min_lower        = 1
  min_upper        = 1
  min_numeric      = 1
  min_special      = 1
}

resource "random_password" "role" {
  for_each = local.roles

  length  = 32
  special = false
}

# ---- Instance --------------------------------------------------------------

resource "aws_db_subnet_group" "this" {
  name        = "${local.name}-db"
  description = "Isolated subnets for the ${var.environment} database"
  subnet_ids  = var.subnet_ids

  tags = merge(var.tags, { Name = "${local.name}-db" })
}

resource "aws_db_parameter_group" "this" {
  name        = "${local.name}-postgres16"
  family      = var.parameter_group_family
  description = "Bowline ${var.environment} PostgreSQL 16"

  parameter {
    name  = "log_min_duration_statement"
    value = tostring(var.log_min_duration_statement_ms)
  }

  parameter {
    name  = "rds.force_ssl"
    value = "1"
  }

  parameter {
    name  = "log_statement"
    value = "ddl"
  }

  tags = var.tags

  lifecycle {
    create_before_destroy = true
  }
}

data "aws_iam_policy_document" "monitoring_assume" {
  statement {
    actions = ["sts:AssumeRole"]

    principals {
      type        = "Service"
      identifiers = ["monitoring.rds.amazonaws.com"]
    }
  }
}

resource "aws_iam_role" "monitoring" {
  count = var.monitoring_interval > 0 ? 1 : 0

  name               = "${local.name}-rds-monitoring"
  assume_role_policy = data.aws_iam_policy_document.monitoring_assume.json

  tags = var.tags
}

resource "aws_iam_role_policy_attachment" "monitoring" {
  count = var.monitoring_interval > 0 ? 1 : 0

  role       = aws_iam_role.monitoring[0].name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonRDSEnhancedMonitoringRole"
}

resource "aws_db_instance" "this" {
  identifier = "${local.name}-postgres"

  engine         = "postgres"
  engine_version = var.engine_version
  instance_class = var.instance_class

  allocated_storage     = var.allocated_storage_gb
  max_allocated_storage = var.max_allocated_storage_gb > 0 ? var.max_allocated_storage_gb : null
  storage_type          = "gp3"
  storage_encrypted     = true
  kms_key_id            = var.kms_key_id

  db_name  = var.database_name
  username = var.master_username
  password = random_password.master.result
  port     = 5432

  db_subnet_group_name   = aws_db_subnet_group.this.name
  vpc_security_group_ids = var.security_group_ids
  parameter_group_name   = aws_db_parameter_group.this.name
  publicly_accessible    = false
  multi_az               = var.multi_az

  backup_retention_period   = var.backup_retention_days
  backup_window             = "03:00-04:00"
  maintenance_window        = "sun:04:30-sun:05:30"
  copy_tags_to_snapshot     = true
  delete_automated_backups  = false
  deletion_protection       = var.deletion_protection
  skip_final_snapshot       = var.skip_final_snapshot
  final_snapshot_identifier = "${local.name}-postgres-final"

  performance_insights_enabled          = var.performance_insights_enabled
  performance_insights_retention_period = var.performance_insights_enabled ? var.performance_insights_retention_days : null
  monitoring_interval                   = var.monitoring_interval
  monitoring_role_arn                   = var.monitoring_interval > 0 ? aws_iam_role.monitoring[0].arn : null
  enabled_cloudwatch_logs_exports       = ["postgresql", "upgrade"]

  auto_minor_version_upgrade = true
  apply_immediately          = var.apply_immediately
  ca_cert_identifier         = "rds-ca-rsa2048-g1"

  tags = merge(var.tags, { Name = "${local.name}-postgres" })
}

# ---- Secrets ---------------------------------------------------------------

resource "aws_secretsmanager_secret" "master" {
  name                    = "bowline/${var.environment}/db/master"
  description             = "RDS master credentials for bowline-${var.environment}. Used by the migrate task only."
  recovery_window_in_days = var.secret_recovery_window_days
  kms_key_id              = var.secrets_kms_key_id

  tags = var.tags
}

resource "aws_secretsmanager_secret_version" "master" {
  secret_id = aws_secretsmanager_secret.master.id

  secret_string = jsonencode({
    engine   = "postgres"
    host     = local.host
    port     = local.port
    dbname   = var.database_name
    username = var.master_username
    password = random_password.master.result
    url      = "postgresql://${var.master_username}:${urlencode(random_password.master.result)}@${local.host}:${local.port}/${var.database_name}?sslmode=require"
    jdbc_url = local.jdbc_url
  })
}

resource "aws_secretsmanager_secret" "role" {
  for_each = local.roles

  name                    = "bowline/${var.environment}/db/${each.key}"
  description             = "PostgreSQL role ${each.value} for bowline-${var.environment}"
  recovery_window_in_days = var.secret_recovery_window_days
  kms_key_id              = var.secrets_kms_key_id

  tags = var.tags
}

resource "aws_secretsmanager_secret_version" "role" {
  for_each = local.roles

  secret_id = aws_secretsmanager_secret.role[each.key].id

  secret_string = jsonencode({
    engine   = "postgres"
    host     = local.host
    port     = local.port
    dbname   = var.database_name
    username = each.value
    password = random_password.role[each.key].result
    url      = "postgresql://${each.value}:${urlencode(random_password.role[each.key].result)}@${local.host}:${local.port}/${var.database_name}?sslmode=require"
    jdbc_url = local.jdbc_url
  })
}
