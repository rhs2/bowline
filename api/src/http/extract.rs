//! Request extractors: validated JSON bodies and query strings, client IP.

use std::convert::Infallible;
use std::net::SocketAddr;

use axum::async_trait;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{ConnectInfo, FromRequest, FromRequestParts, Query, Request};
use axum::http::request::Parts;
use axum::Json;
use serde::de::DeserializeOwned;
use validator::Validate;

use crate::error::ApiError;

/// JSON body that has been deserialised and validated.
pub struct ValidatedJson<T>(pub T);

#[async_trait]
impl<T, S> FromRequest<S> for ValidatedJson<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(req, state)
            .await
            .map_err(json_rejection)?;
        value.validate()?;
        Ok(ValidatedJson(value))
    }
}

fn json_rejection(rej: JsonRejection) -> ApiError {
    match rej {
        JsonRejection::JsonDataError(e) => ApiError::validation("body", e.body_text()),
        JsonRejection::JsonSyntaxError(e) => ApiError::validation("body", e.body_text()),
        JsonRejection::MissingJsonContentType(_) => ApiError::validation(
            "body",
            "expected a JSON body with content-type application/json",
        ),
        other => ApiError::validation("body", other.body_text()),
    }
}

/// Query string that has been deserialised and validated.
pub struct ValidatedQuery<T>(pub T);

#[async_trait]
impl<T, S> FromRequestParts<S> for ValidatedQuery<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Query(value) = Query::<T>::from_request_parts(parts, state)
            .await
            .map_err(|e: QueryRejection| ApiError::validation("query", e.body_text()))?;
        value.validate()?;
        Ok(ValidatedQuery(value))
    }
}

/// The caller's IP: the last `X-Forwarded-For` hop (appended by the load balancer)
/// when present, otherwise the peer address.
pub struct ClientIp(pub Option<String>);

#[async_trait]
impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let forwarded = parts
            .headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next_back())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip().to_string());
        Ok(ClientIp(forwarded.or(peer)))
    }
}
