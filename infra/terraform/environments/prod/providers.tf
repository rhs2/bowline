provider "aws" {
  region = var.aws_region

  default_tags {
    tags = merge(
      {
        Project     = "bowline"
        Environment = var.environment
        ManagedBy   = "terraform"
        Repository  = "github.com/rhs2/bowline"
      },
      var.tags,
    )
  }
}
