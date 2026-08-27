//! Liveness, readiness and Prometheus metrics.

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

use crate::db;
use crate::state::AppState;

#[derive(Serialize, ToSchema)]
pub struct Health {
    pub status: &'static str,
}

#[derive(Serialize, ToSchema)]
pub struct Readiness {
    pub status: &'static str,
    pub database: &'static str,
    pub pending_migrations: usize,
}

#[utoipa::path(get, path = "/healthz", tag = "platform", responses((status = 200, body = Health)))]
pub async fn healthz() -> Json<Health> {
    Json(Health { status: "ok" })
}

#[utoipa::path(get, path = "/readyz", tag = "platform",
    responses((status = 200, body = Readiness), (status = 503, body = Readiness)))]
pub async fn readyz(State(state): State<AppState>) -> impl IntoResponse {
    match db::pending_migrations(&state.pool).await {
        Ok(pending) if pending.is_empty() => (
            StatusCode::OK,
            Json(Readiness {
                status: "ready",
                database: "ok",
                pending_migrations: 0,
            }),
        ),
        Ok(pending) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(Readiness {
                status: "migrations pending",
                database: "ok",
                pending_migrations: pending.len(),
            }),
        ),
        Err(e) => {
            tracing::warn!(error = %e, "readiness check failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(Readiness {
                    status: "database unreachable",
                    database: "error",
                    pending_migrations: 0,
                }),
            )
        }
    }
}

#[utoipa::path(get, path = "/metrics", tag = "platform", responses((status = 200, description = "Prometheus text exposition")))]
pub async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    metrics::gauge!("bowline_db_pool_size").set(state.pool.size() as f64);
    metrics::gauge!("bowline_db_pool_idle").set(state.pool.num_idle() as f64);
    if let Ok(depth) = sqlx::query_scalar::<_, i64>(
        "select count(*) from notifications where status in ('pending','sending')",
    )
    .fetch_one(&state.pool)
    .await
    {
        metrics::gauge!("bowline_outbox_depth").set(depth as f64);
    }
    let body = state
        .metrics
        .as_ref()
        .map(|h| h.render())
        .unwrap_or_default();
    ([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], body)
}
