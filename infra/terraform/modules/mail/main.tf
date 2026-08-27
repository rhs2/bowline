# Amazon SES for the notify worker: a verified domain identity with DKIM, a
# configuration set that requires TLS and publishes delivery metrics, and an
# SMTP credential (IAM user + access key) stored in Secrets Manager.
#
# SES derives the SMTP password from the secret access key with a SigV4-style
# HMAC chain (region-scoped). The aws_iam_access_key resource exposes exactly
# that derivation as ses_smtp_password_v4, so the password never has to be
# computed by hand.

data "aws_region" "current" {}
data "aws_caller_identity" "current" {}

locals {
  name             = "bowline-${var.environment}"
  smtp_host        = "email-smtp.${data.aws_region.current.name}.amazonaws.com"
  mail_from_domain = var.mail_from_subdomain == null ? null : "${var.mail_from_subdomain}.${var.domain_name}"
  dkim_tokens      = aws_sesv2_email_identity.domain.dkim_signing_attributes[0].tokens

  dkim_records = [
    for token in local.dkim_tokens : {
      name  = "${token}._domainkey.${var.domain_name}"
      type  = "CNAME"
      value = "${token}.dkim.amazonses.com"
    }
  ]

  mail_from_records = local.mail_from_domain == null ? [] : [
    {
      name  = local.mail_from_domain
      type  = "MX"
      value = "10 feedback-smtp.${data.aws_region.current.name}.amazonses.com"
    },
    {
      name  = local.mail_from_domain
      type  = "TXT"
      value = "v=spf1 include:amazonses.com -all"
    },
  ]

  dmarc_records = var.dmarc_record == null ? [] : [
    {
      name  = "_dmarc.${var.domain_name}"
      type  = "TXT"
      value = var.dmarc_record
    },
  ]
}

# ---- Configuration set -----------------------------------------------------

resource "aws_sesv2_configuration_set" "this" {
  configuration_set_name = local.name

  delivery_options {
    tls_policy = "REQUIRE"
  }

  reputation_options {
    reputation_metrics_enabled = true
  }

  sending_options {
    sending_enabled = true
  }

  tags = var.tags
}

resource "aws_sesv2_configuration_set_event_destination" "cloudwatch" {
  configuration_set_name = aws_sesv2_configuration_set.this.configuration_set_name
  event_destination_name = "cloudwatch"

  event_destination {
    enabled              = true
    matching_event_types = ["SEND", "DELIVERY", "BOUNCE", "COMPLAINT", "REJECT", "RENDERING_FAILURE"]

    cloud_watch_destination {
      dimension_configuration {
        default_dimension_value = local.name
        dimension_name          = "ses:configuration-set"
        dimension_value_source  = "MESSAGE_TAG"
      }
    }
  }
}

# ---- Identity --------------------------------------------------------------

resource "aws_sesv2_email_identity" "domain" {
  email_identity         = var.domain_name
  configuration_set_name = aws_sesv2_configuration_set.this.configuration_set_name

  dkim_signing_attributes {
    next_signing_key_length = "RSA_2048_BIT"
  }

  tags = var.tags
}

resource "aws_sesv2_email_identity_mail_from_attributes" "domain" {
  count = local.mail_from_domain == null ? 0 : 1

  email_identity         = aws_sesv2_email_identity.domain.email_identity
  behavior_on_mx_failure = "USE_DEFAULT_VALUE"
  mail_from_domain       = local.mail_from_domain
}

# ---- DNS (optional) --------------------------------------------------------

resource "aws_route53_record" "dkim" {
  count = var.route53_zone_id == null ? 0 : 3

  zone_id = var.route53_zone_id
  name    = local.dkim_records[count.index].name
  type    = "CNAME"
  ttl     = 600
  records = [local.dkim_records[count.index].value]
}

resource "aws_route53_record" "mail_from_mx" {
  count = var.route53_zone_id == null || local.mail_from_domain == null ? 0 : 1

  zone_id = var.route53_zone_id
  name    = local.mail_from_domain
  type    = "MX"
  ttl     = 600
  records = ["10 feedback-smtp.${data.aws_region.current.name}.amazonses.com"]
}

resource "aws_route53_record" "mail_from_spf" {
  count = var.route53_zone_id == null || local.mail_from_domain == null ? 0 : 1

  zone_id = var.route53_zone_id
  name    = local.mail_from_domain
  type    = "TXT"
  ttl     = 600
  records = ["v=spf1 include:amazonses.com -all"]
}

resource "aws_route53_record" "dmarc" {
  count = var.route53_zone_id == null || var.dmarc_record == null ? 0 : 1

  zone_id = var.route53_zone_id
  name    = "_dmarc.${var.domain_name}"
  type    = "TXT"
  ttl     = 600
  records = [var.dmarc_record]
}

# ---- SMTP credential -------------------------------------------------------

resource "aws_iam_user" "smtp" {
  name = "${local.name}-ses-smtp"
  path = "/bowline/"

  tags = var.tags
}

data "aws_iam_policy_document" "smtp" {
  statement {
    sid     = "SendThroughVerifiedIdentity"
    actions = ["ses:SendRawEmail", "ses:SendEmail"]
    resources = [
      aws_sesv2_email_identity.domain.arn,
      aws_sesv2_configuration_set.this.arn,
    ]

    condition {
      test     = "StringLike"
      variable = "ses:FromAddress"
      values   = ["*@${var.domain_name}"]
    }
  }
}

resource "aws_iam_user_policy" "smtp" {
  name   = "ses-send"
  user   = aws_iam_user.smtp.name
  policy = data.aws_iam_policy_document.smtp.json
}

resource "aws_iam_access_key" "smtp" {
  user = aws_iam_user.smtp.name
}

resource "aws_secretsmanager_secret" "smtp" {
  name                    = "bowline/${var.environment}/smtp"
  description             = "SES SMTP credentials for the notify worker (bowline-${var.environment})"
  recovery_window_in_days = var.secret_recovery_window_days
  kms_key_id              = var.secrets_kms_key_id

  tags = var.tags
}

resource "aws_secretsmanager_secret_version" "smtp" {
  secret_id = aws_secretsmanager_secret.smtp.id

  secret_string = jsonencode({
    host     = local.smtp_host
    port     = 587
    starttls = "1"
    username = aws_iam_access_key.smtp.id
    password = aws_iam_access_key.smtp.ses_smtp_password_v4
  })
}
