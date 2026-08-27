//! Client for the billing service (invoice PDFs, AR aging spreadsheets).

use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("{service} request failed: {source}")]
    Transport {
        service: &'static str,
        #[source]
        source: reqwest::Error,
    },
    #[error("{service} responded with status {status}")]
    Status { service: &'static str, status: u16 },
}

#[derive(Clone)]
pub struct BillingClient {
    http: reqwest::Client,
    base_url: String,
    token: String,
}

/// What billing answers to a render call: the object it wrote and how big it is.
#[derive(Debug, Clone, Deserialize)]
pub struct Rendered {
    pub s3_key: String,
    #[serde(default)]
    pub bytes: u64,
}

impl BillingClient {
    pub fn new(http: reqwest::Client, base_url: &str, token: &str) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.to_string(),
        }
    }

    /// `POST /render/invoice`: billing derives the key from the invoice number.
    pub async fn render_invoice(&self, payload: &Value) -> Result<Rendered, ClientError> {
        self.render("/render/invoice", payload).await
    }

    /// `POST /render/document`: one personnel file, stored under the key in the body.
    pub async fn render_document(&self, payload: &Value) -> Result<Rendered, ClientError> {
        self.render("/render/document", payload).await
    }

    async fn render(&self, path: &str, payload: &Value) -> Result<Rendered, ClientError> {
        let resp = self
            .http
            .post(format!("{}{path}", self.base_url))
            .header("X-Internal-Token", &self.token)
            .timeout(Duration::from_secs(15))
            .json(payload)
            .send()
            .await
            .map_err(|source| ClientError::Transport {
                service: "billing",
                source,
            })?;
        if !resp.status().is_success() {
            return Err(ClientError::Status {
                service: "billing",
                status: resp.status().as_u16(),
            });
        }
        resp.json().await.map_err(|source| ClientError::Transport {
            service: "billing",
            source,
        })
    }

    /// Liveness probe, so a batch job can fail fast with a useful message instead of
    /// once per row. The probe needs no internal token.
    pub async fn healthz(&self) -> Result<(), ClientError> {
        let resp = self
            .http
            .get(format!("{}/healthz", self.base_url))
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|source| ClientError::Transport {
                service: "billing",
                source,
            })?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(ClientError::Status {
                service: "billing",
                status: resp.status().as_u16(),
            })
        }
    }

    pub async fn ar_aging_xlsx(&self, as_of: &str) -> Result<(Vec<u8>, String), ClientError> {
        let resp = self
            .http
            .get(format!("{}/reports/ar-aging.xlsx", self.base_url))
            .query(&[("as_of", as_of)])
            .header("X-Internal-Token", &self.token)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|source| ClientError::Transport {
                service: "billing",
                source,
            })?;
        if !resp.status().is_success() {
            return Err(ClientError::Status {
                service: "billing",
                status: resp.status().as_u16(),
            });
        }
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet")
            .to_string();
        let bytes = resp
            .bytes()
            .await
            .map_err(|source| ClientError::Transport {
                service: "billing",
                source,
            })?;
        Ok((bytes.to_vec(), content_type))
    }
}
