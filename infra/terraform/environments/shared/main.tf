# Account-level resources shared by every environment: the ECR repositories
# (one image set per commit serves staging and prod at different tags) and the
# GitHub Actions OIDC trust the deploy workflow uses instead of long-lived keys.
#
# Applied once from an operator's machine before the first deploy; the deploy
# role cannot create itself.

data "aws_caller_identity" "current" {}

locals {
  account_id = data.aws_caller_identity.current.account_id

  oidc_provider_arn = var.create_oidc_provider ? aws_iam_openid_connect_provider.github[0].arn : var.oidc_provider_arn

  subjects = concat(
    [for b in var.deploy_branches : "repo:${var.github_repository}:ref:refs/heads/${b}"],
    [for e in var.deploy_environments : "repo:${var.github_repository}:environment:${e}"],
  )
}

module "ecr" {
  source = "../../modules/ecr"

  pull_account_ids = var.pull_account_ids
}

# ---- GitHub OIDC -----------------------------------------------------------

# AWS validates GitHub's certificate chain against its trusted CAs and ignores
# the thumbprint for this issuer, but the API still requires one value.
resource "aws_iam_openid_connect_provider" "github" {
  count = var.create_oidc_provider ? 1 : 0

  url             = "https://token.actions.githubusercontent.com"
  client_id_list  = ["sts.amazonaws.com"]
  thumbprint_list = ["6938fd4d98bab03faadb97b34396831e3780aea1", "1c58a3a8518e8759bf075b76b750d4f2df264fcd"]

  tags = var.tags
}

data "aws_iam_policy_document" "deploy_assume" {
  statement {
    sid     = "GitHubActions"
    actions = ["sts:AssumeRoleWithWebIdentity"]

    principals {
      type        = "Federated"
      identifiers = [local.oidc_provider_arn]
    }

    condition {
      test     = "StringEquals"
      variable = "token.actions.githubusercontent.com:aud"
      values   = ["sts.amazonaws.com"]
    }

    condition {
      test     = "StringLike"
      variable = "token.actions.githubusercontent.com:sub"
      values   = local.subjects
    }
  }
}

data "aws_iam_policy_document" "deploy" {
  statement {
    sid       = "ManageBowlineServices"
    actions   = [for s in var.deploy_role_services : "${s}:*"]
    resources = ["*"]
  }

  statement {
    sid     = "ManageBowlineIam"
    actions = ["iam:*"]
    resources = [
      "arn:aws:iam::${local.account_id}:role/bowline-*",
      "arn:aws:iam::${local.account_id}:policy/bowline-*",
      "arn:aws:iam::${local.account_id}:user/bowline/*",
      "arn:aws:iam::${local.account_id}:instance-profile/bowline-*",
    ]
  }

  statement {
    sid = "ReadIamAndServiceLinkedRoles"
    actions = [
      "iam:Get*",
      "iam:List*",
      "iam:CreateServiceLinkedRole",
    ]
    resources = ["*"]
  }

  statement {
    sid     = "TerraformState"
    actions = ["s3:ListBucket", "s3:GetObject", "s3:PutObject", "s3:DeleteObject"]
    resources = [
      "arn:aws:s3:::${var.state_bucket_name}",
      "arn:aws:s3:::${var.state_bucket_name}/*",
    ]
  }

  statement {
    sid       = "TerraformLock"
    actions   = ["dynamodb:GetItem", "dynamodb:PutItem", "dynamodb:DeleteItem", "dynamodb:DescribeTable"]
    resources = ["arn:aws:dynamodb:*:${local.account_id}:table/${var.state_lock_table_name}"]
  }
}

resource "aws_iam_role" "deploy" {
  name                 = var.deploy_role_name
  description          = "Assumed by github.com/${var.github_repository} deploy workflow through OIDC"
  assume_role_policy   = data.aws_iam_policy_document.deploy_assume.json
  max_session_duration = 3600

  tags = var.tags
}

resource "aws_iam_role_policy" "deploy" {
  name   = "deploy"
  role   = aws_iam_role.deploy.id
  policy = data.aws_iam_policy_document.deploy.json
}
