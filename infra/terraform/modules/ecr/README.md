# ecr

One ECR repository per deployable service (`bowline/api`, `bowline/web`, `bowline/billing`, `bowline/analytics`, `bowline/notify`), scanning on push, AES-256 encryption at rest, and a lifecycle policy that expires untagged images after 7 days and keeps the last 20 images of each repository.

## Applied once per account

The deploy workflow builds one image set per commit on `main` and pushes it as `<registry>/bowline/<service>:<12-char SHA>` plus `:latest`. Staging and production consume the same repositories at different tags, so this module lives in `environments/shared`, not in the per-environment roots. If production runs in its own AWS account, set `pull_account_ids` to that account and it receives a repository policy allowing pulls.

Tags are mutable because the workflow moves `:latest` on every push. Deployments never reference `:latest`; the ECS task definitions pin the SHA tag through the `image_tag` variable, which is what makes rollback a matter of re-applying with the previous tag.

## Inputs

| Name                   | Type         | Default                                     |
|------------------------|--------------|---------------------------------------------|
| `repository_names`     | list(string) | `["api","web","billing","analytics","notify"]` |
| `name_prefix`          | string       | `bowline`                                   |
| `keep_last_images`     | number       | `20`                                        |
| `untagged_expiry_days` | number       | `7`                                         |
| `force_delete`         | bool         | `false`                                     |
| `pull_account_ids`     | list(string) | `[]`                                        |
| `tags`                 | map(string)  | `{}`                                        |

## Outputs

`registry_id`, `registry_url`, `repository_names`, `repository_urls`, `repository_arns`.
