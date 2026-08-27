//! Router assembly and the middleware stack.

use std::time::Duration;

use axum::http::{header, HeaderName, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use axum_prometheus::PrometheusMetricLayer;
use tower_http::catch_panic::CatchPanicLayer;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::request_id::{PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::Level;
use utoipa_scalar::{Scalar, Servable};

use crate::error::{ApiError, Problem};
use crate::state::AppState;

pub mod extract;
pub mod pagination;
pub mod ratelimit;
pub mod request_id;

/// Builds the application router. The metrics layer is passed in rather than
/// created here: it must come from the same `PrometheusMetricLayer::pair()` as the
/// handle held in `AppState`, because building a second recorder installs a second
/// global exporter and panics on the port it tries to open.
pub fn router(state: AppState, metric_layer: Option<PrometheusMetricLayer<'static>>) -> Router {
    let api = Router::new()
        .merge(crate::auth::handlers::routes())
        .merge(crate::org::handlers::routes())
        .merge(crate::hr::handlers::routes())
        .merge(crate::ops::handlers::routes())
        .merge(crate::finance::handlers::routes())
        .merge(crate::comms::handlers::routes())
        .merge(crate::support::handlers::routes())
        .merge(crate::admin::handlers::routes())
        .merge(crate::dashboard::routes())
        .fallback(api_not_found)
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'"),
        ));

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(
            state
                .config
                .cors_origins
                .iter()
                .filter_map(|o| HeaderValue::from_str(o).ok()),
        ))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            HeaderName::from_static(request_id::HEADER),
        ])
        .expose_headers([HeaderName::from_static(request_id::HEADER)])
        .max_age(Duration::from_secs(600));

    let trace = TraceLayer::new_for_http()
        .make_span_with(|req: &axum::http::Request<_>| {
            let request_id = req
                .headers()
                .get(request_id::HEADER)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("-");
            tracing::info_span!(
                "request",
                method = %req.method(),
                path = %req.uri().path(),
                request_id = %request_id,
                user_id = tracing::field::Empty,
            )
        })
        .on_response(DefaultOnResponse::new().level(Level::INFO));

    let mut app = Router::new()
        .route("/healthz", get(crate::health::healthz))
        .route("/readyz", get(crate::health::readyz))
        .route("/metrics", get(crate::health::metrics))
        .route(
            "/api-docs/openapi.json",
            get(|| async { axum::Json(crate::openapi::ApiDoc::document()) }),
        )
        .merge(Scalar::with_url(
            "/docs",
            crate::openapi::ApiDoc::document(),
        ))
        .nest("/api/v1", api)
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_FRAME_OPTIONS,
            HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::REFERRER_POLICY,
            HeaderValue::from_static("no-referrer"),
        ))
        .layer(SetResponseHeaderLayer::if_not_present(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=31536000; includeSubDomains"),
        ))
        .layer(CompressionLayer::new())
        .layer(cors)
        // 504 rather than the layer's default 408: the request was fine, it was
        // this service that ran out of time serving it.
        .layer(TimeoutLayer::with_status_code(
            StatusCode::GATEWAY_TIMEOUT,
            Duration::from_secs(30),
        ))
        .layer(CatchPanicLayer::custom(handle_panic))
        .layer(axum::middleware::from_fn(normalise_errors))
        .layer(axum::middleware::from_fn(request_id::scope_request_id))
        .layer(trace)
        .layer(PropagateRequestIdLayer::new(HeaderName::from_static(
            request_id::HEADER,
        )))
        .layer(SetRequestIdLayer::new(
            HeaderName::from_static(request_id::HEADER),
            request_id::MakeUuidRequestId,
        ));
    if let Some(layer) = metric_layer {
        app = app.layer(layer);
    }
    app.with_state(state)
}

/// Rewrites any error response that is not already a problem document.
///
/// Handlers return `ApiError`, which serialises correctly. What does not are the
/// rejections axum produces before a handler is ever reached: a query string that
/// will not deserialise, an unsupported method, a body over the limit. Those answer
/// in plain text, so a client that parses every error as JSON breaks on them. This
/// gives them the same shape as everything else, and leaves a real problem document
/// untouched.
async fn normalise_errors(req: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let response = next.run(req).await;
    let status = response.status();
    if !(status.is_client_error() || status.is_server_error()) {
        return response;
    }
    let already_problem = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/problem+json"));
    if already_problem {
        return response;
    }

    let (parts, body) = response.into_parts();
    let detail = match axum::body::to_bytes(body, 8 * 1024).await {
        Ok(bytes) if !bytes.is_empty() => String::from_utf8_lossy(&bytes).trim().to_string(),
        _ => status
            .canonical_reason()
            .unwrap_or("request failed")
            .to_string(),
    };
    let code = match status {
        StatusCode::BAD_REQUEST => "validation_failed",
        StatusCode::UNAUTHORIZED => "unauthorized",
        StatusCode::FORBIDDEN => "forbidden",
        StatusCode::NOT_FOUND => "not_found",
        StatusCode::METHOD_NOT_ALLOWED => "method_not_allowed",
        StatusCode::PAYLOAD_TOO_LARGE => "payload_too_large",
        StatusCode::UNSUPPORTED_MEDIA_TYPE => "unsupported_media_type",
        StatusCode::UNPROCESSABLE_ENTITY => "validation_failed",
        StatusCode::TOO_MANY_REQUESTS => "rate_limited",
        _ if status.is_server_error() => "internal",
        _ => "bad_request",
    };
    Problem {
        kind: "about:blank".to_string(),
        title: status.canonical_reason().unwrap_or("Error").to_string(),
        status: parts.status.as_u16(),
        detail,
        code: code.to_string(),
        request_id: request_id::current_request_id(),
        errors: None,
    }
    .into_response()
}

async fn api_not_found() -> Response {
    ApiError::NotFound("route not found".to_string()).into_response()
}

fn handle_panic(err: Box<dyn std::any::Any + Send + 'static>) -> Response {
    let detail = if let Some(s) = err.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = err.downcast_ref::<&str>() {
        s.to_string()
    } else {
        "unknown panic".to_string()
    };
    tracing::error!(panic = %detail, "handler panicked");
    Problem {
        kind: "about:blank".to_string(),
        title: "Internal Server Error".to_string(),
        status: StatusCode::INTERNAL_SERVER_ERROR.as_u16(),
        detail: "an internal error occurred".to_string(),
        code: "internal".to_string(),
        request_id: request_id::current_request_id(),
        errors: None,
    }
    .into_response()
}
