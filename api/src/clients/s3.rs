//! Presigned S3 URLs. Bytes never pass through the API; the browser talks to S3 (or
//! MinIO locally) directly.

use std::time::Duration;

use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
use aws_sdk_s3::presigning::PresigningConfig;

use crate::config::S3Config;
use crate::error::{ApiError, ApiResult};

pub struct S3Client {
    client: aws_sdk_s3::Client,
    /// Used only for the existence probe; see [`S3Client::object_exists`].
    http: reqwest::Client,
    ttl: Duration,
    pub bucket_documents: String,
    pub bucket_pdfs: String,
}

impl S3Client {
    pub fn new(cfg: &S3Config) -> S3Client {
        let credentials = Credentials::new(
            cfg.access_key_id.clone(),
            cfg.secret_access_key.clone(),
            None,
            None,
            "static",
        );
        let mut builder = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new(cfg.region.clone()))
            .credentials_provider(credentials)
            .force_path_style(cfg.force_path_style);
        if let Some(endpoint) = &cfg.endpoint {
            builder = builder.endpoint_url(endpoint.clone());
        }
        S3Client {
            client: aws_sdk_s3::Client::from_conf(builder.build()),
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(3))
                .build()
                .expect("reqwest client"),
            ttl: Duration::from_secs(cfg.presign_ttl_seconds.max(60)),
            bucket_documents: cfg.bucket_documents.clone(),
            bucket_pdfs: cfg.bucket_pdfs.clone(),
        }
    }

    fn presigning(&self) -> ApiResult<PresigningConfig> {
        PresigningConfig::expires_in(self.ttl)
            .map_err(|e| ApiError::internal_msg(format!("presign config: {e}")))
    }

    pub async fn presign_put(
        &self,
        bucket: &str,
        key: &str,
        content_type: &str,
    ) -> ApiResult<String> {
        let presigned = self
            .client
            .put_object()
            .bucket(bucket)
            .key(key)
            .content_type(content_type)
            .presigned(self.presigning()?)
            .await
            .map_err(|e| ApiError::internal_msg(format!("presign put: {e}")))?;
        Ok(presigned.uri().to_string())
    }

    /// True when the object is there.
    ///
    /// Done the same way a download is: presign the request and follow it. The SDK
    /// client in this process signs, it does not transport (bytes never pass through
    /// the API), and a HEAD carries no body in either direction.
    ///
    /// A bucket policy that grants `s3:GetObject` without `s3:ListBucket` answers 403
    /// rather than 404 for a key that is not there, so both mean the same thing here.
    /// A missing object is an answer, not a failure; anything else is an error.
    pub async fn object_exists(&self, bucket: &str, key: &str) -> ApiResult<bool> {
        let presigned = self
            .client
            .head_object()
            .bucket(bucket)
            .key(key)
            .presigned(self.presigning()?)
            .await
            .map_err(|e| ApiError::internal_msg(format!("presign head: {e}")))?;
        let response = self
            .http
            .head(presigned.uri())
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| ApiError::internal_msg(format!("head s3://{bucket}/{key}: {e}")))?;
        match response.status().as_u16() {
            200 => Ok(true),
            404 => Ok(false),
            403 => {
                tracing::warn!(
                    bucket,
                    key,
                    "S3 answered 403 to a HEAD; treating the object as absent"
                );
                Ok(false)
            }
            status => Err(ApiError::internal_msg(format!(
                "head s3://{bucket}/{key} answered {status}"
            ))),
        }
    }

    pub async fn presign_get(&self, bucket: &str, key: &str) -> ApiResult<String> {
        let presigned = self
            .client
            .get_object()
            .bucket(bucket)
            .key(key)
            .presigned(self.presigning()?)
            .await
            .map_err(|e| ApiError::internal_msg(format!("presign get: {e}")))?;
        Ok(presigned.uri().to_string())
    }
}

/// Object keys are built from ids and a sanitised title, never from raw input.
pub fn sanitise_filename(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('_');
    if trimmed.is_empty() {
        "file".to_string()
    } else {
        trimmed.chars().take(80).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn presigns_without_network() {
        let cfg = S3Config {
            endpoint: Some("http://localhost:9000".to_string()),
            region: "us-east-1".to_string(),
            bucket_documents: "bowline-documents".to_string(),
            bucket_pdfs: "bowline-pdfs".to_string(),
            access_key_id: "minioadmin".to_string(),
            secret_access_key: "minioadmin".to_string(),
            force_path_style: true,
            presign_ttl_seconds: 900,
        };
        let client = S3Client::new(&cfg);
        let url = client
            .presign_put(
                "bowline-documents",
                "employees/x/contract.pdf",
                "application/pdf",
            )
            .await
            .unwrap();
        assert!(
            url.starts_with("http://localhost:9000/bowline-documents/employees/x/contract.pdf?")
        );
        assert!(url.contains("X-Amz-Signature="));
        let get = client.presign_get("bowline-pdfs", "a/b.pdf").await.unwrap();
        assert!(get.starts_with("http://localhost:9000/bowline-pdfs/a/b.pdf?"));
    }

    #[test]
    fn filenames_are_safe() {
        assert_eq!(
            sanitise_filename("My Contract (2026).pdf"),
            "My_Contract__2026_.pdf"
        );
        assert_eq!(sanitise_filename("///"), "file");
    }
}
