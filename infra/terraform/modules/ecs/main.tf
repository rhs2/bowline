# ECS Fargate cluster, load balancer, service discovery, IAM, task definitions
# and services for the five Bowline services, plus the one-off migrate task.
#
# Traffic:  browser -> ALB (443) -> web:3000 | api:8080 (path rules)
#           web  -> api.bowline.local:8080            (SSR, Cloud Map)
#           api  -> billing.bowline.local:8081, analytics.bowline.local:8082
#           notify polls the database and talks SMTP to SES.

data "aws_region" "current" {}
data "aws_caller_identity" "current" {}

locals {
  name       = "bowline-${var.environment}"
  region     = data.aws_region.current.name
  account_id = data.aws_caller_identity.current.account_id
  s3_region  = coalesce(var.s3_region, local.region)
  ns         = var.service_discovery_namespace

  public_origin = "https://${var.public_hostname}"

  # ECS task definition families are account-wide, so the family carries the
  # environment. A single shared "bowline-migrate" would resolve to whichever
  # environment was applied most recently, and a staging deploy could then run
  # migrations against the production database. The deploy workflow resolves the
  # same name (FAMILY="bowline-migrate-$ENVIRONMENT").
  migrate_family = coalesce(var.migrate_task_family, "bowline-migrate-${var.environment}")

  service_names      = ["api", "web", "billing", "analytics", "notify"]
  container_ports    = { api = 8080, web = 3000, billing = 8081, analytics = 8082, notify = 9101 }
  lb_services        = { api = { port = 8080, health_path = "/healthz" }, web = { port = 3000, health_path = "/healthz" } }
  internal_services  = ["billing", "analytics", "notify"]
  discovery_services = ["api", "billing", "analytics"]

  image = { for s in local.service_names : s => "${var.ecr_registry}/${var.image_name_prefix}/${s}:${var.image_tag}" }

  # Plain environment. Every name comes from .env.example; secrets are below.
  api_env = {
    API_BIND                   = "0.0.0.0:8080"
    API_PUBLIC_URL             = local.public_origin
    API_CORS_ORIGINS           = local.public_origin
    JWT_ISSUER                 = var.jwt_issuer
    ACCESS_TOKEN_TTL_SECONDS   = tostring(var.access_token_ttl_seconds)
    REFRESH_TOKEN_TTL_SECONDS  = tostring(var.refresh_token_ttl_seconds)
    LOGIN_MAX_FAILURES         = tostring(var.login_max_failures)
    LOGIN_LOCKOUT_SECONDS      = tostring(var.login_lockout_seconds)
    RATE_LIMIT_PER_MINUTE      = tostring(var.rate_limit_per_minute)
    INVOICE_APPROVAL_THRESHOLD = tostring(var.invoice_approval_threshold)
    DATABASE_MAX_CONNECTIONS   = tostring(var.database_max_connections)
    DATABASE_MIGRATE_ON_START  = "0" # migrations are the separate migrate task
    BILLING_URL                = "http://billing.${local.ns}:8081"
    ANALYTICS_URL              = "http://analytics.${local.ns}:8082"
    LOG_FORMAT                 = "json"
    RUST_LOG                   = var.rust_log
    S3_ENDPOINT                = "" # empty: real AWS S3
    S3_REGION                  = local.s3_region
    S3_BUCKET_DOCUMENTS        = var.s3_bucket_names["documents"]
    S3_BUCKET_PDFS             = var.s3_bucket_names["pdfs"]
    S3_ACCESS_KEY_ID           = "" # empty: the task role is the credential
    S3_SECRET_ACCESS_KEY       = ""
    S3_FORCE_PATH_STYLE        = "0"
    PRESIGN_TTL_SECONDS        = tostring(var.presign_ttl_seconds)
  }

  web_env = {
    API_INTERNAL_URL     = "http://api.${local.ns}:8080"
    NEXT_PUBLIC_API_URL  = local.public_origin
    NEXT_PUBLIC_APP_NAME = var.app_name
    PORT                 = "3000"
    HOSTNAME             = "0.0.0.0"
    NODE_ENV             = "production"
  }

  billing_env = {
    BILLING_BIND_PORT       = "8081"
    BILLING_PDF_OUTPUT      = "s3"
    BILLING_COMPANY_NAME    = var.billing_company_name
    BILLING_COMPANY_ADDRESS = var.billing_company_address
    S3_ENDPOINT             = ""
    S3_REGION               = local.s3_region
    S3_BUCKET_PDFS          = var.s3_bucket_names["pdfs"]
    S3_ACCESS_KEY_ID        = ""
    S3_SECRET_ACCESS_KEY    = ""
    S3_FORCE_PATH_STYLE     = "0"
    LOG_FORMAT              = "json"
  }

  analytics_env = {
    ANALYTICS_BIND_PORT  = "8082"
    ANALYTICS_MODEL_PATH = var.analytics_model_path
    LOG_FORMAT           = "json"
  }

  notify_env = {
    SMTP_HOST               = var.smtp_host
    SMTP_PORT               = tostring(var.smtp_port)
    SMTP_STARTTLS           = "1"
    MAIL_FROM               = var.mail_from
    NOTIFY_POLL_INTERVAL_MS = tostring(var.notify_poll_interval_ms)
    NOTIFY_BATCH_SIZE       = tostring(var.notify_batch_size)
    NOTIFY_MAX_ATTEMPTS     = tostring(var.notify_max_attempts)
    NOTIFY_METRICS_BIND     = "0.0.0.0:9101"
    LOG_FORMAT              = "json"
  }

  migrate_env = {
    LOG_FORMAT                = "json"
    RUST_LOG                  = var.rust_log
    DATABASE_MIGRATE_ON_START = "1"
  }

  plain_env = {
    api       = local.api_env
    web       = local.web_env
    billing   = local.billing_env
    analytics = local.analytics_env
    notify    = local.notify_env
    migrate   = local.migrate_env
  }

  environment = {
    for s, env in local.plain_env :
    s => [for k, v in merge(env, try(var.extra_environment[s], {})) : { name = k, value = v }]
  }

  # Secrets: valueFrom is "<secret arn>:<json key>::" for JSON secrets and the
  # bare ARN for plain-string secrets.
  db_app    = var.db_role_secret_arns["app"]
  db_ro     = var.db_role_secret_arns["ro"]
  db_notify = var.db_role_secret_arns["notify"]

  secrets = {
    api = [
      { name = "DATABASE_URL", valueFrom = "${local.db_app}:url::" },
      { name = "REDIS_URL", valueFrom = "${var.redis_secret_arn}:url::" },
      { name = "JWT_SECRET", valueFrom = var.jwt_secret_arn },
      { name = "INTERNAL_SERVICE_TOKEN", valueFrom = var.internal_service_token_secret_arn },
    ]
    web = []
    billing = [
      { name = "BILLING_DATABASE_URL", valueFrom = "${local.db_ro}:jdbc_url::" },
      { name = "BILLING_DATABASE_USER", valueFrom = "${local.db_ro}:username::" },
      { name = "BILLING_DATABASE_PASSWORD", valueFrom = "${local.db_ro}:password::" },
      { name = "INTERNAL_SERVICE_TOKEN", valueFrom = var.internal_service_token_secret_arn },
    ]
    analytics = [
      { name = "ANALYTICS_DATABASE_URL", valueFrom = "${local.db_ro}:url::" },
      { name = "INTERNAL_SERVICE_TOKEN", valueFrom = var.internal_service_token_secret_arn },
    ]
    notify = [
      { name = "DATABASE_URL_NOTIFY", valueFrom = "${local.db_notify}:url::" },
      { name = "SMTP_USERNAME", valueFrom = "${var.smtp_secret_arn}:username::" },
      { name = "SMTP_PASSWORD", valueFrom = "${var.smtp_secret_arn}:password::" },
    ]
    migrate = [
      { name = "DATABASE_URL", valueFrom = "${var.db_master_secret_arn}:url::" },
      { name = "DATABASE_ROLE_PASSWORD_APP", valueFrom = "${local.db_app}:password::" },
      { name = "DATABASE_ROLE_PASSWORD_RO", valueFrom = "${local.db_ro}:password::" },
      { name = "DATABASE_ROLE_PASSWORD_NOTIFY", valueFrom = "${local.db_notify}:password::" },
    ]
  }

  all_secret_arns = distinct(concat(
    [
      var.db_master_secret_arn,
      var.redis_secret_arn,
      var.jwt_secret_arn,
      var.internal_service_token_secret_arn,
      var.smtp_secret_arn,
    ],
    values(var.db_role_secret_arns),
  ))

  # Which task roles may touch which buckets.
  s3_access = {
    api     = { buckets = ["documents", "pdfs"], actions = ["s3:GetObject", "s3:PutObject", "s3:DeleteObject", "s3:AbortMultipartUpload"] }
    billing = { buckets = ["pdfs"], actions = ["s3:GetObject", "s3:PutObject", "s3:AbortMultipartUpload"] }
  }

  task_role_names = toset(concat(local.service_names, ["migrate"]))

  autoscaled = {
    for k, v in var.services : k => v
    if contains(keys(local.lb_services), k) && v.max_count != null
  }
}

# ---- Cluster ---------------------------------------------------------------

resource "aws_ecs_cluster" "this" {
  name = local.name

  setting {
    name  = "containerInsights"
    value = "enabled"
  }

  tags = var.tags
}

resource "aws_ecs_cluster_capacity_providers" "this" {
  cluster_name       = aws_ecs_cluster.this.name
  capacity_providers = ["FARGATE", "FARGATE_SPOT"]

  default_capacity_provider_strategy {
    capacity_provider = "FARGATE"
    weight            = 1
    base              = 0
  }
}

# ---- Service discovery -----------------------------------------------------

resource "aws_service_discovery_private_dns_namespace" "this" {
  name        = local.ns
  vpc         = var.vpc_id
  description = "Bowline ${var.environment} internal service names"

  tags = var.tags
}

resource "aws_service_discovery_service" "this" {
  for_each = toset(local.discovery_services)

  name = each.key

  dns_config {
    namespace_id   = aws_service_discovery_private_dns_namespace.this.id
    routing_policy = "MULTIVALUE"

    dns_records {
      ttl  = 10
      type = "A"
    }
  }

  health_check_custom_config {}

  tags = merge(var.tags, { Service = each.key })
}

# ---- Load balancer ---------------------------------------------------------

resource "aws_lb" "this" {
  name               = "${local.name}-alb"
  internal           = false
  load_balancer_type = "application"
  security_groups    = [var.alb_security_group_id]
  subnets            = var.public_subnet_ids

  drop_invalid_header_fields = true
  enable_deletion_protection = var.alb_deletion_protection
  idle_timeout               = 60

  dynamic "access_logs" {
    for_each = var.alb_access_logs_bucket == null ? [] : [var.alb_access_logs_bucket]

    content {
      bucket  = access_logs.value
      prefix  = local.name
      enabled = true
    }
  }

  tags = merge(var.tags, { Name = "${local.name}-alb" })
}

resource "aws_lb_target_group" "this" {
  for_each = local.lb_services

  name                 = "${local.name}-${each.key}"
  port                 = each.value.port
  protocol             = "HTTP"
  vpc_id               = var.vpc_id
  target_type          = "ip"
  deregistration_delay = 30

  health_check {
    path                = each.value.health_path
    port                = "traffic-port"
    protocol            = "HTTP"
    matcher             = "200"
    interval            = 15
    timeout             = 5
    healthy_threshold   = 2
    unhealthy_threshold = 3
  }

  tags = merge(var.tags, { Service = each.key })
}

resource "aws_lb_listener" "http" {
  load_balancer_arn = aws_lb.this.arn
  port              = 80
  protocol          = "HTTP"

  default_action {
    type = "redirect"

    redirect {
      port        = "443"
      protocol    = "HTTPS"
      status_code = "HTTP_301"
    }
  }

  tags = var.tags
}

resource "aws_lb_listener" "https" {
  load_balancer_arn = aws_lb.this.arn
  port              = 443
  protocol          = "HTTPS"
  ssl_policy        = var.ssl_policy
  certificate_arn   = var.certificate_arn

  default_action {
    type             = "forward"
    target_group_arn = aws_lb_target_group.this["web"].arn
  }

  tags = var.tags
}

# Prometheus endpoints are for the VPC only; never expose them through the ALB.
resource "aws_lb_listener_rule" "block_metrics" {
  listener_arn = aws_lb_listener.https.arn
  priority     = 5

  action {
    type = "fixed-response"

    fixed_response {
      content_type = "text/plain"
      message_body = "not found"
      status_code  = "404"
    }
  }

  condition {
    path_pattern {
      values = ["/metrics"]
    }
  }

  tags = var.tags
}

resource "aws_lb_listener_rule" "api" {
  listener_arn = aws_lb_listener.https.arn
  priority     = 10

  action {
    type             = "forward"
    target_group_arn = aws_lb_target_group.this["api"].arn
  }

  condition {
    path_pattern {
      values = ["/api/*", "/docs", "/docs/*", "/api-docs/*"]
    }
  }

  tags = var.tags
}

resource "aws_route53_record" "app" {
  count = var.route53_zone_id == null ? 0 : 1

  zone_id = var.route53_zone_id
  name    = var.public_hostname
  type    = "A"

  alias {
    name                   = aws_lb.this.dns_name
    zone_id                = aws_lb.this.zone_id
    evaluate_target_health = true
  }
}

# ---- IAM -------------------------------------------------------------------

data "aws_iam_policy_document" "ecs_tasks_assume" {
  statement {
    actions = ["sts:AssumeRole"]

    principals {
      type        = "Service"
      identifiers = ["ecs-tasks.amazonaws.com"]
    }

    condition {
      test     = "StringEquals"
      variable = "aws:SourceAccount"
      values   = [local.account_id]
    }

    condition {
      test     = "ArnLike"
      variable = "aws:SourceArn"
      values   = ["arn:aws:ecs:${local.region}:${local.account_id}:*"]
    }
  }
}

# Execution role: pulls images, ships logs, resolves secrets into the task.
resource "aws_iam_role" "execution" {
  name               = "${local.name}-ecs-execution"
  assume_role_policy = data.aws_iam_policy_document.ecs_tasks_assume.json

  tags = var.tags
}

resource "aws_iam_role_policy_attachment" "execution_managed" {
  role       = aws_iam_role.execution.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"
}

data "aws_iam_policy_document" "execution" {
  statement {
    sid       = "ReadTaskSecrets"
    actions   = ["secretsmanager:GetSecretValue"]
    resources = local.all_secret_arns
  }

  dynamic "statement" {
    for_each = var.secrets_kms_key_arn == null ? [] : [var.secrets_kms_key_arn]

    content {
      sid       = "DecryptTaskSecrets"
      actions   = ["kms:Decrypt"]
      resources = [statement.value]
    }
  }
}

resource "aws_iam_role_policy" "execution" {
  name   = "task-secrets"
  role   = aws_iam_role.execution.id
  policy = data.aws_iam_policy_document.execution.json
}

# Task roles: one per service so permissions stay per service.
resource "aws_iam_role" "task" {
  for_each = local.task_role_names

  name               = "${local.name}-${each.key}-task"
  assume_role_policy = data.aws_iam_policy_document.ecs_tasks_assume.json

  tags = merge(var.tags, { Service = each.key })
}

data "aws_iam_policy_document" "s3" {
  for_each = local.s3_access

  statement {
    sid       = "Objects"
    actions   = each.value.actions
    resources = [for b in each.value.buckets : "${var.s3_bucket_arns[b]}/*"]
  }

  statement {
    sid       = "ListBucket"
    actions   = ["s3:ListBucket"]
    resources = [for b in each.value.buckets : var.s3_bucket_arns[b]]
  }

  statement {
    sid       = "ObjectEncryption"
    actions   = ["kms:GenerateDataKey", "kms:Decrypt"]
    resources = [var.s3_kms_key_arn]
  }
}

resource "aws_iam_role_policy" "s3" {
  for_each = local.s3_access

  name   = "s3-access"
  role   = aws_iam_role.task[each.key].id
  policy = data.aws_iam_policy_document.s3[each.key].json
}

data "aws_iam_policy_document" "ses_send" {
  statement {
    sid       = "SendMail"
    actions   = ["ses:SendEmail", "ses:SendRawEmail"]
    resources = [var.ses_identity_arn, var.ses_configuration_set_arn]
  }
}

resource "aws_iam_role_policy" "notify_ses" {
  name   = "ses-send"
  role   = aws_iam_role.task["notify"].id
  policy = data.aws_iam_policy_document.ses_send.json
}

data "aws_iam_policy_document" "execute_command" {
  statement {
    sid = "EcsExec"
    actions = [
      "ssmmessages:CreateControlChannel",
      "ssmmessages:CreateDataChannel",
      "ssmmessages:OpenControlChannel",
      "ssmmessages:OpenDataChannel",
    ]
    resources = ["*"]
  }
}

resource "aws_iam_role_policy" "execute_command" {
  for_each = var.enable_execute_command ? local.task_role_names : toset([])

  name   = "ecs-exec"
  role   = aws_iam_role.task[each.key].id
  policy = data.aws_iam_policy_document.execute_command.json
}

# ---- Task definitions ------------------------------------------------------

resource "aws_ecs_task_definition" "service" {
  for_each = var.services

  family                   = "${local.name}-${each.key}"
  network_mode             = "awsvpc"
  requires_compatibilities = ["FARGATE"]
  cpu                      = tostring(each.value.cpu)
  memory                   = tostring(each.value.memory)
  execution_role_arn       = aws_iam_role.execution.arn
  task_role_arn            = aws_iam_role.task[each.key].arn

  runtime_platform {
    operating_system_family = "LINUX"
    cpu_architecture        = "X86_64"
  }

  container_definitions = jsonencode([
    {
      name      = each.key
      image     = local.image[each.key]
      essential = true

      portMappings = [
        { containerPort = local.container_ports[each.key], protocol = "tcp" }
      ]

      environment = local.environment[each.key]
      secrets     = local.secrets[each.key]

      logConfiguration = {
        logDriver = "awslogs"
        options = {
          "awslogs-group"         = var.log_group_names[each.key]
          "awslogs-region"        = local.region
          "awslogs-stream-prefix" = each.key
        }
      }

      linuxParameters = { initProcessEnabled = true }
      stopTimeout     = 30
    }
  ])

  tags = merge(var.tags, { Service = each.key })
}

# One-off migration task: the api image with the `migrate` command, master
# database credentials, and the three role passwords so it can create the
# application roles on first run. Started by the deploy workflow with
# `aws ecs run-task`, never as a service.
resource "aws_ecs_task_definition" "migrate" {
  family                   = local.migrate_family
  network_mode             = "awsvpc"
  requires_compatibilities = ["FARGATE"]
  cpu                      = tostring(var.migrate_cpu)
  memory                   = tostring(var.migrate_memory)
  execution_role_arn       = aws_iam_role.execution.arn
  task_role_arn            = aws_iam_role.task["migrate"].arn

  runtime_platform {
    operating_system_family = "LINUX"
    cpu_architecture        = "X86_64"
  }

  container_definitions = jsonencode([
    {
      name      = "migrate"
      image     = local.image["api"]
      essential = true
      command   = ["migrate"]

      environment = local.environment["migrate"]
      secrets     = local.secrets["migrate"]

      logConfiguration = {
        logDriver = "awslogs"
        options = {
          "awslogs-group"         = var.log_group_names["migrate"]
          "awslogs-region"        = local.region
          "awslogs-stream-prefix" = "migrate"
        }
      }

      linuxParameters = { initProcessEnabled = true }
    }
  ])

  tags = merge(var.tags, { Service = "migrate" })
}

resource "aws_ssm_parameter" "migrate_network" {
  name        = "/bowline/${var.environment}/migrate-network"
  description = "Network configuration for `aws ecs run-task --task-definition ${local.migrate_family}` in cluster ${local.name}"
  type        = "String"

  value = jsonencode({
    awsvpcConfiguration = {
      subnets        = var.private_subnet_ids
      securityGroups = [var.ecs_security_group_id]
      assignPublicIp = "DISABLED"
    }
  })

  tags = var.tags
}

# ---- Services --------------------------------------------------------------

resource "aws_ecs_service" "public" {
  for_each = local.lb_services

  name             = "${local.name}-${each.key}"
  cluster          = aws_ecs_cluster.this.id
  task_definition  = aws_ecs_task_definition.service[each.key].arn
  desired_count    = var.services[each.key].desired_count
  launch_type      = "FARGATE"
  platform_version = "LATEST"

  deployment_minimum_healthy_percent = 100
  deployment_maximum_percent         = 200
  health_check_grace_period_seconds  = 60
  enable_execute_command             = var.enable_execute_command
  propagate_tags                     = "SERVICE"

  deployment_circuit_breaker {
    enable   = true
    rollback = true
  }

  network_configuration {
    subnets          = var.private_subnet_ids
    security_groups  = [var.ecs_security_group_id]
    assign_public_ip = false
  }

  load_balancer {
    target_group_arn = aws_lb_target_group.this[each.key].arn
    container_name   = each.key
    container_port   = each.value.port
  }

  dynamic "service_registries" {
    for_each = contains(local.discovery_services, each.key) ? [each.key] : []

    content {
      registry_arn = aws_service_discovery_service.this[service_registries.value].arn
    }
  }

  tags = merge(var.tags, { Service = each.key })

  # Autoscaling owns desired_count after the first apply.
  lifecycle {
    ignore_changes = [desired_count]
  }

  depends_on = [aws_lb_listener.https, aws_lb_listener_rule.api]
}

resource "aws_ecs_service" "internal" {
  for_each = toset(local.internal_services)

  name             = "${local.name}-${each.key}"
  cluster          = aws_ecs_cluster.this.id
  task_definition  = aws_ecs_task_definition.service[each.key].arn
  desired_count    = var.services[each.key].desired_count
  launch_type      = "FARGATE"
  platform_version = "LATEST"

  deployment_minimum_healthy_percent = 100
  deployment_maximum_percent         = 200
  enable_execute_command             = var.enable_execute_command
  propagate_tags                     = "SERVICE"

  deployment_circuit_breaker {
    enable   = true
    rollback = true
  }

  network_configuration {
    subnets          = var.private_subnet_ids
    security_groups  = [var.ecs_security_group_id]
    assign_public_ip = false
  }

  dynamic "service_registries" {
    for_each = contains(local.discovery_services, each.key) ? [each.key] : []

    content {
      registry_arn = aws_service_discovery_service.this[service_registries.value].arn
    }
  }

  tags = merge(var.tags, { Service = each.key })
}

# ---- Autoscaling (api, web) ------------------------------------------------

resource "aws_appautoscaling_target" "this" {
  for_each = local.autoscaled

  service_namespace  = "ecs"
  scalable_dimension = "ecs:service:DesiredCount"
  resource_id        = "service/${aws_ecs_cluster.this.name}/${aws_ecs_service.public[each.key].name}"
  min_capacity       = coalesce(each.value.min_count, each.value.desired_count)
  max_capacity       = each.value.max_count

  tags = merge(var.tags, { Service = each.key })
}

resource "aws_appautoscaling_policy" "cpu" {
  for_each = local.autoscaled

  name               = "${local.name}-${each.key}-cpu"
  policy_type        = "TargetTrackingScaling"
  service_namespace  = aws_appautoscaling_target.this[each.key].service_namespace
  scalable_dimension = aws_appautoscaling_target.this[each.key].scalable_dimension
  resource_id        = aws_appautoscaling_target.this[each.key].resource_id

  target_tracking_scaling_policy_configuration {
    target_value       = each.value.cpu_target_percent
    scale_in_cooldown  = 300
    scale_out_cooldown = 60

    predefined_metric_specification {
      predefined_metric_type = "ECSServiceAverageCPUUtilization"
    }
  }
}
