output "bucket_names" {
  description = "Bucket names keyed by documents and pdfs."
  value       = { for k, b in aws_s3_bucket.this : k => b.bucket }
}

output "bucket_arns" {
  description = "Bucket ARNs keyed by documents and pdfs."
  value       = { for k, b in aws_s3_bucket.this : k => b.arn }
}

output "documents_bucket_name" {
  description = "Name of the documents bucket (S3_BUCKET_DOCUMENTS)."
  value       = aws_s3_bucket.this["documents"].bucket
}

output "pdfs_bucket_name" {
  description = "Name of the PDFs bucket (S3_BUCKET_PDFS)."
  value       = aws_s3_bucket.this["pdfs"].bucket
}

output "kms_key_arn" {
  description = "KMS key used for SSE-KMS. Task roles that presign uploads need kms:GenerateDataKey and kms:Decrypt on it."
  value       = local.kms_key_arn
}
