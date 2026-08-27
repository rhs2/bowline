variable "repository_names" {
  description = "One repository per deployable service. The deploy workflow pushes <registry>/<name_prefix>/<service>:<tag>."
  type        = list(string)
  default     = ["api", "web", "billing", "analytics", "notify"]
}

variable "name_prefix" {
  description = "Namespace prefix of every repository (bowline/api, bowline/web, ...). Must match the deploy workflow."
  type        = string
  default     = "bowline"
}

variable "keep_last_images" {
  description = "Number of most recent images to keep per repository; older ones are expired by the lifecycle policy."
  type        = number
  default     = 20
}

variable "untagged_expiry_days" {
  description = "Untagged images (layers left behind by a re-tag) are expired after this many days."
  type        = number
  default     = 7
}

variable "force_delete" {
  description = "Allow terraform destroy to remove repositories that still contain images."
  type        = bool
  default     = false
}

variable "pull_account_ids" {
  description = "Other AWS account ids allowed to pull images, for a setup where production runs in a separate account from the one that builds images."
  type        = list(string)
  default     = []
}

variable "tags" {
  description = "Additional tags applied to every resource in this module."
  type        = map(string)
  default     = {}
}
