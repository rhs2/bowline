# Remote state. The bucket and lock table are created once by hand (see
# infra/README.md, "Bootstrapping the state backend"); replace the account id
# placeholder with the real one or pass -backend-config at init.
terraform {
  backend "s3" {
    bucket         = "bowline-terraform-state-000000000000"
    key            = "staging/terraform.tfstate"
    region         = "us-east-1"
    dynamodb_table = "bowline-terraform-locks"
    encrypt        = true
  }
}
