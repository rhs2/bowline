# Staging: one NAT gateway, single-AZ database, one Redis node, small tasks,
# short retention, quick deletion. Same modules and wiring as production.

data "aws_caller_identity" "current" {}
data "aws_region" "current" {}

locals {
  environment     = var.environment
  public_hostname = coalesce(var.public_hostname, "staging.${var.domain_name}")
  mail_domain     = coalesce(var.mail_domain, "staging.${var.domain_name}")
  mail_from       = coalesce(var.mail_from, "Bowline Staging <no-reply@${local.mail_domain}>")
  public_origin   = "https://${local.public_hostname}"
  ecr_registry    = "${data.aws_caller_identity.current.account_id}.dkr.ecr.${data.aws_region.current.name}.amazonaws.com"
}

module "network" {
  source = "../../modules/network"

  environment                = local.environment
  vpc_cidr                   = var.vpc_cidr
  az_count                   = 2
  single_nat_gateway         = true
  enable_interface_endpoints = false
  enable_flow_logs           = true
  flow_log_retention_days    = 7
}

module "secrets" {
  source = "../../modules/secrets"

  environment                 = local.environment
  secret_recovery_window_days = 0
}

module "database" {
  source = "../../modules/database"

  environment        = local.environment
  subnet_ids         = module.network.isolated_subnet_ids
  security_group_ids = [module.network.db_security_group_id]

  instance_class              = "db.t4g.medium"
  allocated_storage_gb        = 20
  max_allocated_storage_gb    = 100
  multi_az                    = false
  backup_retention_days       = 7
  deletion_protection         = false
  skip_final_snapshot         = true
  monitoring_interval         = 60
  apply_immediately           = true
  secret_recovery_window_days = 0
}

module "cache" {
  source = "../../modules/cache"

  environment        = local.environment
  subnet_ids         = module.network.isolated_subnet_ids
  security_group_ids = [module.network.cache_security_group_id]

  node_type                   = "cache.t4g.micro"
  num_cache_clusters          = 1
  automatic_failover_enabled  = false
  multi_az_enabled            = false
  apply_immediately           = true
  secret_recovery_window_days = 0
}

module "storage" {
  source = "../../modules/storage"

  environment          = local.environment
  cors_allowed_origins = [local.public_origin]
  force_destroy        = true
}

module "mail" {
  source = "../../modules/mail"

  environment                 = local.environment
  domain_name                 = local.mail_domain
  route53_zone_id             = var.route53_zone_id
  dmarc_record                = var.dmarc_record
  secret_recovery_window_days = 0
}

module "observability" {
  source = "../../modules/observability"

  environment        = local.environment
  log_retention_days = 14
  alarm_email        = var.alarm_email

  alb_arn_suffix            = module.ecs.alb_arn_suffix
  target_group_arn_suffixes = module.ecs.target_group_arn_suffixes
  db_instance_identifier    = module.database.instance_identifier
  ecs_cluster_name          = module.ecs.cluster_name
  ecs_service_names         = module.ecs.service_names
}

module "ecs" {
  source = "../../modules/ecs"

  environment           = local.environment
  vpc_id                = module.network.vpc_id
  public_subnet_ids     = module.network.public_subnet_ids
  private_subnet_ids    = module.network.private_subnet_ids
  alb_security_group_id = module.network.alb_security_group_id
  ecs_security_group_id = module.network.ecs_security_group_id

  public_hostname         = local.public_hostname
  certificate_arn         = var.certificate_arn
  route53_zone_id         = var.route53_zone_id
  alb_deletion_protection = false

  ecr_registry = local.ecr_registry
  image_tag    = var.image_tag

  services = {
    api       = { cpu = 256, memory = 512, desired_count = 1, min_count = 1, max_count = 2 }
    web       = { cpu = 256, memory = 512, desired_count = 1, min_count = 1, max_count = 2 }
    billing   = { cpu = 512, memory = 1024, desired_count = 1 }
    analytics = { cpu = 256, memory = 512, desired_count = 1 }
    notify    = { cpu = 256, memory = 512, desired_count = 1 }
  }

  enable_execute_command = var.enable_execute_command
  log_group_names        = module.observability.log_group_names

  db_master_secret_arn              = module.database.master_secret_arn
  db_role_secret_arns               = module.database.role_secret_arns
  redis_secret_arn                  = module.cache.secret_arn
  jwt_secret_arn                    = module.secrets.jwt_secret_arn
  internal_service_token_secret_arn = module.secrets.internal_service_token_arn
  smtp_secret_arn                   = module.mail.smtp_secret_arn

  s3_bucket_names = module.storage.bucket_names
  s3_bucket_arns  = module.storage.bucket_arns
  s3_kms_key_arn  = module.storage.kms_key_arn

  ses_identity_arn          = module.mail.identity_arn
  ses_configuration_set_arn = module.mail.configuration_set_arn
  smtp_host                 = module.mail.smtp_host
  smtp_port                 = module.mail.smtp_port
  mail_from                 = local.mail_from

  rust_log = "info,bowline_api=debug,sqlx=warn"
}
