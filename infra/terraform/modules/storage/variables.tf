variable "environment" {
  description = "Deployment stage name (staging, prod)."
  type        = string
}

variable "bucket_name_suffix" {
  description = "Suffix that makes the bucket names globally unique. Null uses the AWS account id, giving bowline-<env>-documents-<account id>."
  type        = string
  default     = null
}

variable "cors_allowed_origins" {
  description = "Browser origins allowed to PUT and GET objects through presigned URLs, for example [\"https://app.bowline.example\"]."
  type        = list(string)
}

variable "kms_key_arn" {
  description = "Customer managed KMS key for SSE-KMS. Null creates a dedicated key with annual rotation."
  type        = string
  default     = null
}

variable "noncurrent_version_expiration_days" {
  description = "Days after which non-current object versions (overwritten or deleted) are permanently removed."
  type        = number
  default     = 90
}

variable "abort_incomplete_multipart_days" {
  description = "Days after which abandoned multipart uploads are aborted."
  type        = number
  default     = 7
}

variable "force_destroy" {
  description = "Allow terraform destroy to empty the buckets first. Keep false in production."
  type        = bool
  default     = false
}

variable "tags" {
  description = "Additional tags applied to every resource in this module."
  type        = map(string)
  default     = {}
}
