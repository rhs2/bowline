//! Client for the analytics service. Delay-risk scoring fails open: a missing score
//! never blocks a booking.

use std::time::Duration;

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct DelayRiskRequest {
    pub mode: String,
    pub weight_kg: Decimal,
    pub pieces: i32,
    pub hazardous: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_km: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub carrier_on_time_rate: Option<Decimal>,
    pub etd: Option<NaiveDate>,
    pub eta: Option<NaiveDate>,
}

#[derive(Debug, Deserialize)]
struct DelayRiskResponse {
    risk: f64,
}

#[derive(Clone)]
pub struct AnalyticsClient {
    http: reqwest::Client,
    base_url: String,
    token: String,
}

impl AnalyticsClient {
    pub fn new(http: reqwest::Client, base_url: &str, token: &str) -> Self {
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.to_string(),
        }
    }

    pub async fn delay_risk(&self, req: &DelayRiskRequest) -> Option<Decimal> {
        let result = self
            .http
            .post(format!("{}/score/delay-risk", self.base_url))
            .header("X-Internal-Token", &self.token)
            .timeout(Duration::from_secs(3))
            .json(req)
            .send()
            .await;
        let resp = match result {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                tracing::warn!(
                    status = r.status().as_u16(),
                    "analytics rejected delay-risk request"
                );
                return None;
            }
            Err(e) => {
                tracing::warn!(error = %e, "analytics unreachable; shipment saved without a score");
                return None;
            }
        };
        match resp.json::<DelayRiskResponse>().await {
            Ok(body) if body.risk.is_finite() => Decimal::try_from(body.risk.clamp(0.0, 1.0))
                .ok()
                .map(|d| d.round_dp(4)),
            _ => {
                tracing::warn!("analytics returned an unreadable delay-risk score");
                None
            }
        }
    }
}
