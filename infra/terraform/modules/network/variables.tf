variable "environment" {
  description = "Deployment stage name (staging, prod). Used in every resource name and in the CloudWatch log group path."
  type        = string

  validation {
    condition     = can(regex("^[a-z][a-z0-9-]{1,15}$", var.environment))
    error_message = "environment must be 2 to 16 lowercase letters, digits or hyphens and start with a letter."
  }
}

variable "vpc_cidr" {
  description = "CIDR block for the VPC. It is split into /20 subnets: public (ALB), private (ECS tasks) and isolated (RDS, ElastiCache), one of each per availability zone."
  type        = string
  default     = "10.0.0.0/16"

  validation {
    condition     = can(cidrhost(var.vpc_cidr, 0)) && tonumber(split("/", var.vpc_cidr)[1]) <= 20
    error_message = "vpc_cidr must be a valid IPv4 CIDR no smaller than /20 so it can hold twelve /20 subnets."
  }
}

variable "az_count" {
  description = "Number of availability zones to spread the subnets over (2 or 3)."
  type        = number
  default     = 2

  validation {
    condition     = var.az_count >= 2 && var.az_count <= 3
    error_message = "az_count must be 2 or 3."
  }
}

variable "single_nat_gateway" {
  description = "When true, one NAT gateway serves every private subnet (cheaper, one AZ of egress failure domain). When false, one NAT gateway per availability zone."
  type        = bool
  default     = true
}

variable "enable_interface_endpoints" {
  description = "Create interface VPC endpoints for ECR (api and dkr), CloudWatch Logs and Secrets Manager so image pulls, log shipping and secret reads never leave the VPC. The S3 gateway endpoint is always created because it is free. Interface endpoints cost roughly USD 7 per endpoint per AZ per month; staging may disable them and route through NAT instead."
  type        = bool
  default     = true
}

variable "enable_flow_logs" {
  description = "Capture VPC flow logs (all traffic) into a CloudWatch log group."
  type        = bool
  default     = true
}

variable "flow_log_retention_days" {
  description = "Retention of the VPC flow log group in days."
  type        = number
  default     = 30
}

variable "tags" {
  description = "Additional tags applied to every resource in this module."
  type        = map(string)
  default     = {}
}
