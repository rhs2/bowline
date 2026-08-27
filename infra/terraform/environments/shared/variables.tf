variable "aws_region" {
  description = "Region of the ECR registry and the IAM resources' home region."
  type        = string
  default     = "us-east-1"
}

variable "github_repository" {
  description = "GitHub repository (owner/name) whose workflows may assume the deploy role."
  type        = string
  default     = "rhs2/bowline"
}

variable "deploy_branches" {
  description = "Branches allowed to assume the deploy role from a plain push (the images job). Environment-scoped jobs are matched by deploy_environments."
  type        = list(string)
  default     = ["main"]
}

variable "deploy_environments" {
  description = "GitHub environment names allowed to assume the deploy role (the terraform and migrate jobs)."
  type        = list(string)
  default     = ["staging", "prod"]
}

variable "create_oidc_provider" {
  description = "Create the GitHub Actions OIDC provider. An account can hold only one provider for token.actions.githubusercontent.com; set false and pass oidc_provider_arn if it already exists."
  type        = bool
  default     = true
}

variable "oidc_provider_arn" {
  description = "ARN of an existing GitHub OIDC provider, used when create_oidc_provider is false."
  type        = string
  default     = null
}

variable "deploy_role_name" {
  description = "Name of the role the deploy workflow assumes (AWS_DEPLOY_ROLE_ARN secret)."
  type        = string
  default     = "bowline-github-deploy"
}

variable "deploy_role_services" {
  description = "AWS service prefixes the deploy role may fully manage. IAM is handled separately and restricted to bowline-* resources."
  type        = list(string)
  default = [
    "acm",
    "application-autoscaling",
    "cloudwatch",
    "dynamodb",
    "ec2",
    "ecr",
    "ecs",
    "elasticache",
    "elasticloadbalancing",
    "kms",
    "logs",
    "rds",
    "route53",
    "s3",
    "secretsmanager",
    "servicediscovery",
    "ses",
    "sns",
    "ssm",
  ]
}

variable "state_bucket_name" {
  description = "Name of the Terraform state bucket, so the deploy role can read and write state. Must match backend.tf in every root."
  type        = string
  default     = "bowline-terraform-state-000000000000"
}

variable "state_lock_table_name" {
  description = "Name of the DynamoDB lock table. Must match backend.tf in every root."
  type        = string
  default     = "bowline-terraform-locks"
}

variable "pull_account_ids" {
  description = "Other account ids allowed to pull images (a separate production account, for example)."
  type        = list(string)
  default     = []
}

variable "tags" {
  description = "Extra default tags for every resource."
  type        = map(string)
  default     = {}
}
