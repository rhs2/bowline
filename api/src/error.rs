//! RFC 7807 problem responses with the stable error codes from docs/API.md.

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use utoipa::ToSchema;

use crate::http::request_id::current_request_id;

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct FieldError {
    pub field: String,
    pub message: String,
}

impl FieldError {
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("validation failed")]
    Validation(Vec<FieldError>),
    #[error("{0}")]
    Unauthorized(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    InvalidTransition(String),
    #[error("{0}")]
    Locked(String),
    #[error("rate limit exceeded")]
    RateLimited,
    #[error("internal error: {0}")]
    Internal(Box<dyn std::error::Error + Send + Sync + 'static>),
}

impl ApiError {
    pub fn validation(field: impl Into<String>, message: impl Into<String>) -> Self {
        ApiError::Validation(vec![FieldError::new(field, message)])
    }

    pub fn not_found(what: &str) -> Self {
        ApiError::NotFound(format!("{what} not found"))
    }

    pub fn forbidden(msg: impl Into<String>) -> Self {
        ApiError::Forbidden(msg.into())
    }

    pub fn conflict(msg: impl Into<String>) -> Self {
        ApiError::Conflict(msg.into())
    }

    pub fn transition(from: &str, to: &str) -> Self {
        ApiError::InvalidTransition(format!("cannot transition from {from} to {to}"))
    }

    pub fn internal(err: impl std::error::Error + Send + Sync + 'static) -> Self {
        ApiError::Internal(Box::new(err))
    }

    pub fn internal_msg(msg: impl Into<String>) -> Self {
        ApiError::Internal(msg.into().into())
    }

    pub fn status(&self) -> StatusCode {
        match self {
            ApiError::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            ApiError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            ApiError::Forbidden(_) => StatusCode::FORBIDDEN,
            ApiError::NotFound(_) => StatusCode::NOT_FOUND,
            ApiError::Conflict(_) | ApiError::InvalidTransition(_) => StatusCode::CONFLICT,
            ApiError::Locked(_) => StatusCode::LOCKED,
            ApiError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            ApiError::Validation(_) => "validation_failed",
            ApiError::Unauthorized(_) => "unauthorized",
            ApiError::Forbidden(_) => "forbidden",
            ApiError::NotFound(_) => "not_found",
            ApiError::Conflict(_) => "conflict",
            ApiError::InvalidTransition(_) => "invalid_transition",
            ApiError::Locked(_) => "locked",
            ApiError::RateLimited => "rate_limited",
            ApiError::Internal(_) => "internal",
        }
    }

    fn title(&self) -> &'static str {
        match self {
            ApiError::Validation(_) => "Unprocessable Entity",
            ApiError::Unauthorized(_) => "Unauthorized",
            ApiError::Forbidden(_) => "Forbidden",
            ApiError::NotFound(_) => "Not Found",
            ApiError::Conflict(_) | ApiError::InvalidTransition(_) => "Conflict",
            ApiError::Locked(_) => "Locked",
            ApiError::RateLimited => "Too Many Requests",
            ApiError::Internal(_) => "Internal Server Error",
        }
    }

    fn detail(&self) -> String {
        match self {
            ApiError::Validation(errors) => {
                let mut msg = String::from("request validation failed");
                if let Some(first) = errors.first() {
                    msg.push_str(": ");
                    msg.push_str(&first.field);
                    msg.push(' ');
                    msg.push_str(&first.message);
                }
                msg
            }
            ApiError::Internal(_) => "an internal error occurred".to_string(),
            other => other.to_string(),
        }
    }
}

/// Wire shape of every error response.
#[derive(Debug, Serialize, ToSchema)]
pub struct Problem {
    #[serde(rename = "type")]
    pub kind: String,
    pub title: String,
    pub status: u16,
    pub detail: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<FieldError>>,
}

impl Problem {
    pub fn from_error(err: &ApiError) -> Problem {
        Problem {
            kind: "about:blank".to_string(),
            title: err.title().to_string(),
            status: err.status().as_u16(),
            detail: err.detail(),
            code: err.code().to_string(),
            request_id: current_request_id(),
            errors: match err {
                ApiError::Validation(errors) => Some(errors.clone()),
                _ => None,
            },
        }
    }

    pub fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = serde_json::to_vec(&self).unwrap_or_default();
        let mut resp = (status, body).into_response();
        resp.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        resp
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let ApiError::Internal(source) = &self {
            tracing::error!(error = %source, "request failed");
        }
        Problem::from_error(&self).into_response()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        match &err {
            sqlx::Error::RowNotFound => ApiError::NotFound("resource not found".to_string()),
            sqlx::Error::Database(db) => {
                let code = db.code().map(|c| c.to_string()).unwrap_or_default();
                let message = db.message().to_string();
                match code.as_str() {
                    "23505" => ApiError::Conflict(format!("duplicate value: {message}")),
                    "23P01" => ApiError::Conflict(format!("overlapping value: {message}")),
                    "23514" | "P0001" => ApiError::Conflict(message),
                    "23503" => ApiError::validation(
                        db.constraint().unwrap_or("reference").to_string(),
                        "referenced row does not exist",
                    ),
                    "23502" => ApiError::validation(
                        "body".to_string(),
                        format!("missing required value: {message}"),
                    ),
                    "22P02" | "22007" | "22008" | "22003" => {
                        ApiError::validation("body".to_string(), message)
                    }
                    _ => ApiError::Internal(Box::new(err)),
                }
            }
            _ => ApiError::Internal(Box::new(err)),
        }
    }
}

impl From<validator::ValidationErrors> for ApiError {
    fn from(errors: validator::ValidationErrors) -> Self {
        let mut out = Vec::new();
        flatten_validation("", &errors, &mut out);
        if out.is_empty() {
            out.push(FieldError::new("body", "invalid request"));
        }
        ApiError::Validation(out)
    }
}

fn flatten_validation(
    prefix: &str,
    errors: &validator::ValidationErrors,
    out: &mut Vec<FieldError>,
) {
    use validator::ValidationErrorsKind;
    for (field, kind) in errors.errors() {
        let name = if prefix.is_empty() {
            field.to_string()
        } else {
            format!("{prefix}.{field}")
        };
        match kind {
            ValidationErrorsKind::Field(list) => {
                for e in list {
                    let message = e
                        .message
                        .as_ref()
                        .map(|m| m.to_string())
                        .unwrap_or_else(|| format!("failed {} validation", e.code));
                    out.push(FieldError::new(name.clone(), message));
                }
            }
            ValidationErrorsKind::Struct(inner) => flatten_validation(&name, inner, out),
            ValidationErrorsKind::List(map) => {
                for (idx, inner) in map {
                    flatten_validation(&format!("{name}[{idx}]"), inner, out);
                }
            }
        }
    }
}
