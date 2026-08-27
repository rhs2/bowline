# Log groups, alarms and the notification topic for one environment.
#
# Alarm creation is gated by literal booleans (create_alb_alarms,
# create_rds_alarms) rather than by null checks on the identifiers, because
# the identifiers come from other modules and are unknown on the first plan.

locals {
  name      = "bowline-${var.environment}"
  namespace = coalesce(var.custom_metric_namespace, "Bowline/${var.environment}")

  log_groups = toset(concat(var.service_names, ["migrate"]))

  alarm_actions = [aws_sns_topic.alarms.arn]
}

# ---- Log groups ------------------------------------------------------------

resource "aws_cloudwatch_log_group" "service" {
  for_each = local.log_groups

  name              = "/bowline/${var.environment}/${each.key}"
  retention_in_days = var.log_retention_days
  kms_key_id        = var.logs_kms_key_id

  tags = merge(var.tags, { Service = each.key })
}

# ---- Notification topic ----------------------------------------------------

resource "aws_sns_topic" "alarms" {
  name = "${local.name}-alarms"

  tags = var.tags
}

resource "aws_sns_topic_subscription" "email" {
  count = var.alarm_email == "" ? 0 : 1

  topic_arn = aws_sns_topic.alarms.arn
  protocol  = "email"
  endpoint  = var.alarm_email
}

# ---- Load balancer ---------------------------------------------------------

resource "aws_cloudwatch_metric_alarm" "alb_5xx_rate" {
  count = var.create_alb_alarms ? 1 : 0

  alarm_name          = "${local.name}-alb-5xx-rate"
  alarm_description   = "Target 5xx responses above ${var.alb_5xx_rate_threshold_percent}% of requests. Check api and web logs, then recent deployments."
  comparison_operator = "GreaterThanThreshold"
  threshold           = var.alb_5xx_rate_threshold_percent
  evaluation_periods  = 3
  datapoints_to_alarm = 2
  treat_missing_data  = "notBreaching"

  metric_query {
    id = "requests"

    metric {
      namespace   = "AWS/ApplicationELB"
      metric_name = "RequestCount"
      period      = 60
      stat        = "Sum"

      dimensions = {
        LoadBalancer = var.alb_arn_suffix
      }
    }
  }

  metric_query {
    id = "errors"

    metric {
      namespace   = "AWS/ApplicationELB"
      metric_name = "HTTPCode_Target_5XX_Count"
      period      = 60
      stat        = "Sum"

      dimensions = {
        LoadBalancer = var.alb_arn_suffix
      }
    }
  }

  metric_query {
    id          = "rate"
    expression  = "IF(requests > 0, errors / requests * 100, 0)"
    label       = "Target 5xx percent"
    return_data = true
  }

  alarm_actions = local.alarm_actions
  ok_actions    = local.alarm_actions

  tags = var.tags
}

resource "aws_cloudwatch_metric_alarm" "target_unhealthy" {
  for_each = var.create_alb_alarms ? var.target_group_arn_suffixes : {}

  alarm_name          = "${local.name}-${each.key}-unhealthy-targets"
  alarm_description   = "At least one ${each.key} task is failing its /healthz check. Check the service events and task logs."
  namespace           = "AWS/ApplicationELB"
  metric_name         = "UnHealthyHostCount"
  statistic           = "Maximum"
  period              = 60
  evaluation_periods  = 3
  comparison_operator = "GreaterThanThreshold"
  threshold           = 0
  treat_missing_data  = "notBreaching"

  dimensions = {
    LoadBalancer = var.alb_arn_suffix
    TargetGroup  = each.value
  }

  alarm_actions = local.alarm_actions
  ok_actions    = local.alarm_actions

  tags = var.tags
}

# ---- Database --------------------------------------------------------------

resource "aws_cloudwatch_metric_alarm" "rds_cpu" {
  count = var.create_rds_alarms ? 1 : 0

  alarm_name          = "${local.name}-rds-cpu"
  alarm_description   = "RDS CPU above ${var.rds_cpu_threshold_percent}% for 15 minutes. Look at Performance Insights for the top statements."
  namespace           = "AWS/RDS"
  metric_name         = "CPUUtilization"
  statistic           = "Average"
  period              = 300
  evaluation_periods  = 3
  comparison_operator = "GreaterThanThreshold"
  threshold           = var.rds_cpu_threshold_percent
  treat_missing_data  = "missing"

  dimensions = {
    DBInstanceIdentifier = var.db_instance_identifier
  }

  alarm_actions = local.alarm_actions
  ok_actions    = local.alarm_actions

  tags = var.tags
}

resource "aws_cloudwatch_metric_alarm" "rds_free_storage" {
  count = var.create_rds_alarms ? 1 : 0

  alarm_name          = "${local.name}-rds-free-storage"
  alarm_description   = "RDS free storage under ${floor(var.rds_free_storage_threshold_bytes / 1073741824)} GiB. Storage autoscaling should be growing the volume; if it hit max_allocated_storage_gb, raise it."
  namespace           = "AWS/RDS"
  metric_name         = "FreeStorageSpace"
  statistic           = "Minimum"
  period              = 300
  evaluation_periods  = 2
  comparison_operator = "LessThanThreshold"
  threshold           = var.rds_free_storage_threshold_bytes
  treat_missing_data  = "missing"

  dimensions = {
    DBInstanceIdentifier = var.db_instance_identifier
  }

  alarm_actions = local.alarm_actions
  ok_actions    = local.alarm_actions

  tags = var.tags
}

# ---- ECS services ----------------------------------------------------------

# Container Insights publishes DesiredTaskCount and RunningTaskCount per
# service. Alarm when running stays below desired for five minutes, which is
# longer than any healthy rolling deployment takes to converge.
resource "aws_cloudwatch_metric_alarm" "ecs_running_below_desired" {
  for_each = var.ecs_service_names

  alarm_name          = "${local.name}-${each.key}-running-below-desired"
  alarm_description   = "${each.key}: running tasks below desired count for 5 minutes. Check service events for stopped-task reasons (image pull, secret access, failed health checks)."
  comparison_operator = "GreaterThanThreshold"
  threshold           = 0
  evaluation_periods  = 5
  datapoints_to_alarm = 5
  treat_missing_data  = "notBreaching"

  metric_query {
    id = "desired"

    metric {
      namespace   = "ECS/ContainerInsights"
      metric_name = "DesiredTaskCount"
      period      = 60
      stat        = "Average"

      dimensions = {
        ClusterName = var.ecs_cluster_name
        ServiceName = each.value
      }
    }
  }

  metric_query {
    id = "running"

    metric {
      namespace   = "ECS/ContainerInsights"
      metric_name = "RunningTaskCount"
      period      = 60
      stat        = "Average"

      dimensions = {
        ClusterName = var.ecs_cluster_name
        ServiceName = each.value
      }
    }
  }

  metric_query {
    id          = "missing"
    expression  = "desired - running"
    label       = "Missing tasks"
    return_data = true
  }

  alarm_actions = local.alarm_actions
  ok_actions    = local.alarm_actions

  tags = var.tags
}

# ---- Outbox depth ----------------------------------------------------------

# The notify worker writes a JSON heartbeat line on every poll cycle that
# includes outbox_depth (rows in notifications still pending). A metric filter
# turns that field into a CloudWatch metric in the environment's namespace.
resource "aws_cloudwatch_log_metric_filter" "outbox_depth" {
  name           = "${local.name}-outbox-depth"
  log_group_name = aws_cloudwatch_log_group.service["notify"].name
  pattern        = "{ $.outbox_depth = * }"

  metric_transformation {
    name          = "OutboxDepth"
    namespace     = local.namespace
    value         = "$.outbox_depth"
    default_value = 0
    unit          = "Count"
  }
}

resource "aws_cloudwatch_metric_alarm" "outbox_depth" {
  alarm_name          = "${local.name}-outbox-depth"
  alarm_description   = "More than ${var.outbox_depth_threshold} notifications pending for 15 minutes. Either notify is down, SES is rejecting (check bounce/complaint metrics and the sandbox status), or a burst broadcast is draining."
  namespace           = local.namespace
  metric_name         = "OutboxDepth"
  statistic           = "Maximum"
  period              = 300
  evaluation_periods  = 3
  comparison_operator = "GreaterThanThreshold"
  threshold           = var.outbox_depth_threshold
  treat_missing_data  = "notBreaching"

  alarm_actions = local.alarm_actions
  ok_actions    = local.alarm_actions

  tags = var.tags

  depends_on = [aws_cloudwatch_log_metric_filter.outbox_depth]
}
