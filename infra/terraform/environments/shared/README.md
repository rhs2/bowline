# environments/shared

Account-level resources that both environments depend on and that must exist before the deploy workflow can run:

- the ECR repositories (`modules/ecr`), because `deploy.yml` pushes one image set per commit to `<registry>/bowline/<service>` regardless of the target environment;
- the GitHub Actions OIDC provider and the `bowline-github-deploy` role the workflow assumes.

It is applied once, by an operator with administrative credentials, and rarely touched again.

## Deploy role

Trust: `sts:AssumeRoleWithWebIdentity` from `token.actions.githubusercontent.com` with audience `sts.amazonaws.com`, restricted to subjects `repo:rhs2/bowline:ref:refs/heads/main` (the `images` job) and `repo:rhs2/bowline:environment:staging` / `:prod` (the `terraform` and `migrate` jobs, which run inside GitHub environments). Any other repository, branch or fork is refused at the token exchange.

Permissions: full control over the AWS services the modules use (`deploy_role_services`), IAM restricted to resources named `bowline-*` (roles, policies, instance profiles) or under the `/bowline/` user path, read-only IAM elsewhere, and read/write on the state bucket and lock table. It is deliberately not `AdministratorAccess`: the role cannot create IAM users outside `/bowline/`, cannot touch roles that do not start with `bowline-`, and cannot read other state.

The full trust policy is reproduced in `infra/README.md`.

## Usage

```
cd infra/terraform/environments/shared
terraform init
terraform apply
terraform output deploy_role_arn   # -> GitHub repository secret AWS_DEPLOY_ROLE_ARN
```

If the account already has a GitHub OIDC provider (there can be only one per issuer), set `create_oidc_provider = false` and pass its ARN.

## Outputs

`deploy_role_arn`, `oidc_provider_arn`, `registry_url`, `repository_urls`.
