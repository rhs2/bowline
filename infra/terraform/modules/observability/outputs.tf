output "log_group_names" {
  description = "CloudWatch log group names keyed by service (api, web, billing, analytics, notify, migrate)."
  value       = { for k, g in aws_cloudwatch_log_group.service : k => g.name }
}

output "log_group_arns" {
  description = "CloudWatch log group ARNs keyed by service."
  value       = { for k, g in aws_cloudwatch_log_group.service : k => g.arn }
}

output "sns_topic_arn" {
  description = "ARN of the alarm topic. Subscribe additional endpoints (PagerDuty, Slack) here."
  value       = aws_sns_topic.alarms.arn
}

output "custom_metric_namespace" {
  description = "Namespace of the log-derived metrics (OutboxDepth)."
  value       = local.namespace
}

output "alarm_names" {
  description = "Names of every alarm created, for dashboards and runbooks."
  value = concat(
    [for a in aws_cloudwatch_metric_alarm.alb_5xx_rate : a.alarm_name],
    [for a in aws_cloudwatch_metric_alarm.target_unhealthy : a.alarm_name],
    [for a in aws_cloudwatch_metric_alarm.rds_cpu : a.alarm_name],
    [for a in aws_cloudwatch_metric_alarm.rds_free_storage : a.alarm_name],
    [for a in aws_cloudwatch_metric_alarm.ecs_running_below_desired : a.alarm_name],
    [aws_cloudwatch_metric_alarm.outbox_depth.alarm_name],
  )
}
