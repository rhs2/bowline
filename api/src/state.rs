//! Shared application state handed to every handler.

use std::sync::Arc;
use std::time::Duration;

use axum_prometheus::metrics_exporter_prometheus::PrometheusHandle;
use sqlx::PgPool;

use crate::auth::principal::PrincipalCache;
use crate::clients::analytics::AnalyticsClient;
use crate::clients::billing::BillingClient;
use crate::clients::s3::S3Client;
use crate::config::Config;
use crate::http::ratelimit::RateLimiter;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Arc<Config>,
    pub principals: PrincipalCache,
    pub limiter: Arc<RateLimiter>,
    pub s3: Arc<S3Client>,
    pub billing: BillingClient,
    pub analytics: AnalyticsClient,
    pub metrics: Option<PrometheusHandle>,
}

impl AppState {
    pub async fn build(
        config: Config,
        pool: PgPool,
        metrics: Option<PrometheusHandle>,
    ) -> AppState {
        let redis = match &config.redis_url {
            Some(url) => connect_redis(url).await,
            None => None,
        };
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .build()
            .expect("reqwest client");
        AppState {
            principals: PrincipalCache::new(redis),
            limiter: Arc::new(RateLimiter::new(config.rate_limit_per_minute)),
            s3: Arc::new(S3Client::new(&config.s3)),
            billing: BillingClient::new(
                http.clone(),
                &config.billing_url,
                &config.internal_service_token,
            ),
            analytics: AnalyticsClient::new(
                http,
                &config.analytics_url,
                &config.internal_service_token,
            ),
            pool,
            config: Arc::new(config),
            metrics,
        }
    }
}

/// Redis is an accelerator, not a dependency: when it is missing or unreachable the
/// process falls back to its in-memory cache and keeps serving.
async fn connect_redis(url: &str) -> Option<redis::aio::ConnectionManager> {
    let client = match redis::Client::open(url) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "invalid REDIS_URL; using in-memory cache");
            return None;
        }
    };
    match tokio::time::timeout(Duration::from_secs(2), client.get_connection_manager()).await {
        Ok(Ok(manager)) => {
            tracing::info!("redis connected");
            Some(manager)
        }
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "redis unreachable; using in-memory cache");
            None
        }
        Err(_) => {
            tracing::warn!("redis connection timed out; using in-memory cache");
            None
        }
    }
}
