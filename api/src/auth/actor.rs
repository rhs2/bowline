//! The authenticated caller, resolved once per request.

use std::sync::Arc;

use axum::async_trait;
use axum::extract::{FromRef, FromRequestParts};
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use uuid::Uuid;

use crate::audit::AuditCtx;
use crate::auth::jwt;
use crate::auth::principal::{self, Principal};
use crate::error::{ApiError, ApiResult};
use crate::http::extract::ClientIp;
use crate::http::request_id::current_request_id;
use crate::scope::{Scope, ScopeFilter};
use crate::state::AppState;

/// Routes that stay reachable while a password change is still required.
const PASSWORD_CHANGE_ALLOWED: &[&str] = &[
    "/api/v1/auth/me",
    "/api/v1/auth/change-password",
    "/api/v1/auth/logout",
];

pub struct Actor {
    pub principal: Arc<Principal>,
    pub ip: Option<String>,
}

impl Actor {
    pub fn me(&self) -> Uuid {
        self.principal.employee_id
    }

    pub fn user_id(&self) -> Uuid {
        self.principal.user_id
    }

    pub fn has(&self, permission: &str) -> bool {
        self.principal.has(permission)
    }

    pub fn require(&self, permission: &str) -> ApiResult<()> {
        self.principal.require(permission)
    }

    pub fn require_any(&self, permissions: &[&str]) -> ApiResult<()> {
        self.principal.require_any(permissions)
    }

    pub fn scope(&self, family: &str) -> Option<Scope> {
        self.principal.scope(family)
    }

    /// Scope for a family, or 403 when the principal holds nothing in it.
    pub fn scope_filter(&self, family: &str) -> ApiResult<ScopeFilter> {
        let scope = self
            .principal
            .scope(family)
            .ok_or_else(|| ApiError::Forbidden(format!("requires a {family} permission")))?;
        Ok(ScopeFilter::new(&self.principal, scope))
    }

    /// Scope for a family, defaulting to the caller's own record.
    pub fn scope_filter_or_self(&self, family: &str) -> ScopeFilter {
        let scope = self.principal.scope(family).unwrap_or(Scope::Own);
        ScopeFilter::new(&self.principal, scope)
    }

    pub fn filter(&self, scope: Scope) -> ScopeFilter {
        ScopeFilter::new(&self.principal, scope)
    }

    pub fn audit(&self) -> AuditCtx {
        AuditCtx {
            user_id: Some(self.principal.user_id),
            employee_id: Some(self.principal.employee_id),
            ip: self.ip.clone(),
            request_id: current_request_id(),
        }
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for Actor
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app = AppState::from_ref(state);
        let ip = ClientIp::from_request_parts(parts, state)
            .await
            .map(|c| c.0)
            .unwrap_or(None);
        let token = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| {
                v.strip_prefix("Bearer ")
                    .or_else(|| v.strip_prefix("bearer "))
            })
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| ApiError::Unauthorized("missing bearer token".to_string()))?;
        let claims = jwt::decode_access(&app.config, token)?;
        let principal =
            principal::resolve(&app.pool, &app.principals, claims.sub, claims.tv).await?;
        match principal.user_status.as_str() {
            "active" => {}
            "locked" => return Err(ApiError::Locked("account is locked".to_string())),
            _ => return Err(ApiError::Unauthorized("account is disabled".to_string())),
        }
        if principal.employee_status == "terminated" {
            return Err(ApiError::Unauthorized("employee is terminated".to_string()));
        }
        if !app.limiter.allow(&format!("user:{}", principal.user_id)) {
            return Err(ApiError::RateLimited);
        }
        if principal.must_change_password && !PASSWORD_CHANGE_ALLOWED.contains(&parts.uri.path()) {
            return Err(ApiError::Forbidden(
                "password change required before using the API".to_string(),
            ));
        }
        tracing::Span::current().record("user_id", tracing::field::display(principal.user_id));
        Ok(Actor { principal, ip })
    }
}
