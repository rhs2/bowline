# mail

Amazon SES for outbound mail from the `notify` worker: a verified domain identity with Easy DKIM (2048-bit), a custom MAIL FROM domain, a configuration set that requires TLS and publishes delivery events to CloudWatch, optional Route 53 records, and an SMTP credential in Secrets Manager.

## SMTP credential

`notify` speaks SMTP (Mailpit locally, SES in production), so it needs an SES SMTP user name and password rather than IAM role credentials. The module creates an IAM user `bowline-<env>-ses-smtp` whose only permission is `ses:SendEmail` and `ses:SendRawEmail` through this identity and configuration set, and only with a `From` address at the verified domain. Its access key id is the SMTP user name; the SMTP password is derived from the secret access key by the region-scoped HMAC chain SES documents, which the `aws_iam_access_key` resource exposes as `ses_smtp_password_v4`. Both land in `bowline/<env>/smtp`:

```json
{ "host": "email-smtp.us-east-1.amazonaws.com", "port": 587, "starttls": "1", "username": "AKIA...", "password": "..." }
```

The ecs module injects `SMTP_USERNAME` and `SMTP_PASSWORD` from that secret, and `SMTP_HOST`, `SMTP_PORT`, `SMTP_STARTTLS` and `MAIL_FROM` as plain environment variables. The notify task role additionally gets the same `ses:Send*` permission, so switching the worker to the SES API later requires no infrastructure change.

Rotate the credential with `terraform apply -replace=module.mail.aws_iam_access_key.smtp` and redeploy `notify`.

## DNS

Set `route53_zone_id` and the module creates the three DKIM CNAMEs, the MAIL FROM MX and SPF TXT records, and (when `dmarc_record` is set) the DMARC TXT. Without a zone id the same records are listed in the `dns_records` output so they can be created wherever the domain is hosted. SES marks the identity verified once the DKIM CNAMEs resolve.

## One identity per domain per account

SES identities are account-wide within a region, so staging and production in the same account must use different domains. The environments use `staging.bowline.example` and `bowline.example` respectively; `MAIL_FROM` follows the same split.

## Sandbox

A new account starts in the SES sandbox: only verified addresses receive mail and the quota is 200 messages per day. Request production access from the SES console before the first real deploy; the outbox worker retries with backoff, so nothing is lost while the request is pending.

## Inputs

| Name                          | Type        | Default  |
|-------------------------------|-------------|----------|
| `environment`                 | string      |          |
| `domain_name`                 | string      |          |
| `mail_from_subdomain`         | string      | `mail`   |
| `route53_zone_id`             | string      | `null`   |
| `dmarc_record`                | string      | `null`   |
| `secret_recovery_window_days` | number      | `7`      |
| `secrets_kms_key_id`          | string      | `null`   |
| `tags`                        | map(string) | `{}`     |

## Outputs

`domain_name`, `identity_arn`, `configuration_set_name`, `configuration_set_arn`, `dkim_tokens`, `dns_records`, `mail_from_domain`, `smtp_host`, `smtp_port`, `smtp_user_name`, `smtp_secret_arn`, `smtp_secret_name`.
