# storage

Two private S3 buckets per environment, `documents` (employee and shipment files) and `pdfs` (rendered invoices, statements, spreadsheets), with versioning, SSE-KMS, all public access blocked, lifecycle rules and CORS for presigned browser uploads.

## Naming

`bowline-<env>-documents-<suffix>` and `bowline-<env>-pdfs-<suffix>`, where the suffix defaults to the AWS account id so names are globally unique without inventing anything. The names are outputs and feed `S3_BUCKET_DOCUMENTS` and `S3_BUCKET_PDFS` in the ECS task definitions.

## Controls

| Control                  | Setting                                                            |
|--------------------------|--------------------------------------------------------------------|
| Ownership                | `BucketOwnerEnforced` (ACLs disabled)                              |
| Public access            | all four blocks on                                                 |
| Versioning               | enabled                                                            |
| Encryption               | SSE-KMS with a dedicated rotating key (or `kms_key_arn`), bucket keys on |
| Lifecycle                | non-current versions expire after 90 days; abandoned multipart uploads aborted after 7 |
| CORS                     | `PUT`, `GET`, `HEAD` from `cors_allowed_origins`, exposes `ETag`   |
| Bucket policy            | deny every request without TLS                                     |

Because uploads are presigned by the API's task role, the browser inherits that role's permissions for a single key for `PRESIGN_TTL_SECONDS`. The role needs `kms:GenerateDataKey` on the key for SSE-KMS uploads to succeed; the ecs module grants it from the `kms_key_arn` output.

## Inputs

| Name                                 | Type         | Default   |
|--------------------------------------|--------------|-----------|
| `environment`                        | string       |           |
| `bucket_name_suffix`                 | string       | `null`    |
| `cors_allowed_origins`               | list(string) |           |
| `kms_key_arn`                        | string       | `null`    |
| `noncurrent_version_expiration_days` | number       | `90`      |
| `abort_incomplete_multipart_days`    | number       | `7`       |
| `force_destroy`                      | bool         | `false`   |
| `tags`                               | map(string)  | `{}`      |

## Outputs

`bucket_names`, `bucket_arns`, `documents_bucket_name`, `pdfs_bucket_name`, `kms_key_arn`.
