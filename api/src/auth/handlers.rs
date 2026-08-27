//! Login, refresh rotation with reuse detection, logout, password change, `me`.

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use utoipa::{OpenApi, ToSchema};
use uuid::Uuid;
use validator::Validate;

use crate::audit::{self, AuditCtx};
use crate::auth::actor::Actor;
use crate::auth::{jwt, password, tokens};
use crate::error::{ApiError, ApiResult};
use crate::http::extract::{ClientIp, ValidatedJson};
use crate::http::request_id::current_request_id;
use crate::org::service::{self as org, ChainEntry};
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct LoginRequest {
    #[validate(email)]
    pub email: String,
    #[validate(length(min = 1, max = 256))]
    pub password: String,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct RefreshRequest {
    #[validate(length(min = 32, max = 128))]
    pub refresh_token: String,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct ChangePasswordRequest {
    #[validate(length(min = 1, max = 256))]
    pub current_password: String,
    #[validate(length(min = 1, max = 256))]
    pub new_password: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: &'static str,
    pub expires_in: u64,
    pub must_change_password: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MeUser {
    pub id: Uuid,
    pub email: String,
    pub status: String,
    pub must_change_password: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MeEmployee {
    pub id: Uuid,
    pub employee_no: String,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub title: String,
    pub level: i16,
    pub position_id: Uuid,
    pub department_id: Uuid,
    pub department_name: String,
    pub manager_id: Option<Uuid>,
    pub site: Option<String>,
    pub status: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MeResponse {
    pub user: MeUser,
    pub employee: MeEmployee,
    pub roles: Vec<String>,
    pub permissions: Vec<String>,
    pub chain: Vec<ChainEntry>,
}

#[derive(sqlx::FromRow)]
struct UserAuthRow {
    id: Uuid,
    employee_id: Uuid,
    password_hash: String,
    status: String,
    failed_logins: i32,
    locked_until: Option<DateTime<Utc>>,
    must_change_password: bool,
    token_version: i32,
    employee_status: String,
}

async fn load_user_by_email(
    conn: &mut PgConnection,
    email: &str,
) -> sqlx::Result<Option<UserAuthRow>> {
    sqlx::query_as(
        "select u.id, u.employee_id, u.password_hash, u.status, u.failed_logins, u.locked_until,
                u.must_change_password, u.token_version, e.status as employee_status
           from users u join employees e on e.id = u.employee_id
          where u.email = $1::citext",
    )
    .bind(email)
    .fetch_optional(conn)
    .await
}

async fn load_user_by_id(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<Option<UserAuthRow>> {
    sqlx::query_as(
        "select u.id, u.employee_id, u.password_hash, u.status, u.failed_logins, u.locked_until,
                u.must_change_password, u.token_version, e.status as employee_status
           from users u join employees e on e.id = u.employee_id
          where u.id = $1",
    )
    .bind(id)
    .fetch_optional(conn)
    .await
}

fn ensure_usable(user: &UserAuthRow) -> ApiResult<()> {
    if user.status == "locked" {
        return Err(ApiError::Locked("account is locked".to_string()));
    }
    if let Some(until) = user.locked_until {
        if until > Utc::now() {
            return Err(ApiError::Locked(format!(
                "account is locked until {}",
                until.to_rfc3339()
            )));
        }
    }
    if user.status != "active" || user.employee_status == "terminated" {
        return Err(ApiError::Unauthorized("account is disabled".to_string()));
    }
    Ok(())
}

/// Creates a refresh token (new family or an existing one) and an access token.
pub async fn issue_session(
    conn: &mut PgConnection,
    state: &AppState,
    user_id: Uuid,
    token_version: i32,
    family_id: Uuid,
    ip: Option<&str>,
    must_change_password: bool,
) -> ApiResult<(TokenResponse, Uuid)> {
    let refresh = tokens::generate_refresh();
    let expires_at = Utc::now() + Duration::seconds(state.config.refresh_token_ttl_seconds as i64);
    let ip = ip.filter(|ip| ip.parse::<std::net::IpAddr>().is_ok());
    let token_id: Uuid = sqlx::query_scalar(
        "insert into refresh_tokens (user_id, family_id, token_hash, expires_at, ip)
         values ($1, $2, $3, $4, $5::inet) returning id",
    )
    .bind(user_id)
    .bind(family_id)
    .bind(tokens::hash_refresh(&refresh))
    .bind(expires_at)
    .bind(ip)
    .fetch_one(&mut *conn)
    .await?;
    let access = jwt::encode_access(&state.config, user_id, token_version)?;
    Ok((
        TokenResponse {
            access_token: access,
            refresh_token: refresh,
            token_type: "Bearer",
            expires_in: state.config.access_token_ttl_seconds,
            must_change_password,
        },
        token_id,
    ))
}

#[utoipa::path(post, path = "/api/v1/auth/login", tag = "auth", request_body = LoginRequest,
    responses((status = 200, body = TokenResponse), (status = 401, body = crate::error::Problem),
              (status = 423, body = crate::error::Problem), (status = 429, body = crate::error::Problem)))]
pub async fn login(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ValidatedJson(body): ValidatedJson<LoginRequest>,
) -> ApiResult<Json<TokenResponse>> {
    let ip_key = ip.clone().unwrap_or_else(|| "unknown".to_string());
    if !state.limiter.allow(&format!("ip:{ip_key}")) {
        return Err(ApiError::RateLimited);
    }
    let mut tx = state.pool.begin().await?;
    let Some(user) = load_user_by_email(&mut tx, &body.email).await? else {
        // Burn comparable time so a missing account is not distinguishable by latency.
        let _ = password::verify_async(body.password.clone(), DUMMY_HASH.to_string()).await;
        return Err(ApiError::Unauthorized(
            "invalid email or password".to_string(),
        ));
    };
    ensure_usable(&user)?;
    let ctx = AuditCtx {
        user_id: Some(user.id),
        employee_id: Some(user.employee_id),
        ip: ip.clone(),
        request_id: current_request_id(),
    };
    let ok = password::verify_async(body.password.clone(), user.password_hash.clone()).await?;
    if !ok {
        let failures = user.failed_logins + 1;
        let lock = failures >= state.config.login_max_failures;
        let locked_until =
            lock.then(|| Utc::now() + Duration::seconds(state.config.login_lockout_seconds));
        sqlx::query(
            "update users set failed_logins = case when $2 then 0 else $3 end, locked_until = $4 where id = $1",
        )
        .bind(user.id)
        .bind(lock)
        .bind(failures)
        .bind(locked_until)
        .execute(&mut *tx)
        .await?;
        let action = if lock {
            "auth.lockout"
        } else {
            "auth.login_failed"
        };
        audit::record(
            &mut tx,
            &ctx,
            action,
            "user",
            Some(user.id),
            None,
            Some(serde_json::json!({"failed_logins": failures, "locked_until": locked_until})),
        )
        .await?;
        tx.commit().await?;
        if lock {
            return Err(ApiError::Locked(format!(
                "too many failed logins; locked for {} seconds",
                state.config.login_lockout_seconds
            )));
        }
        return Err(ApiError::Unauthorized(
            "invalid email or password".to_string(),
        ));
    }
    sqlx::query("update users set failed_logins = 0, locked_until = null, last_login_at = now() where id = $1")
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    let (response, _) = issue_session(
        &mut tx,
        &state,
        user.id,
        user.token_version,
        Uuid::new_v4(),
        ip.as_deref(),
        user.must_change_password,
    )
    .await?;
    audit::record(
        &mut tx,
        &ctx,
        "auth.login",
        "user",
        Some(user.id),
        None,
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(Json(response))
}

const DUMMY_HASH: &str = "$argon2id$v=19$m=65536,t=3,p=1$c2FsdHNhbHRzYWx0c2FsdA$Zm9vYmFyYmF6cXV4Zm9vYmFyYmF6cXV4Zm9vYmFyYmF6cXV4";

#[derive(sqlx::FromRow)]
struct RefreshRow {
    id: Uuid,
    user_id: Uuid,
    family_id: Uuid,
    expires_at: DateTime<Utc>,
    revoked_at: Option<DateTime<Utc>>,
}

#[utoipa::path(post, path = "/api/v1/auth/refresh", tag = "auth", request_body = RefreshRequest,
    responses((status = 200, body = TokenResponse), (status = 401, body = crate::error::Problem)))]
pub async fn refresh(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    ValidatedJson(body): ValidatedJson<RefreshRequest>,
) -> ApiResult<Json<TokenResponse>> {
    let ip_key = ip.clone().unwrap_or_else(|| "unknown".to_string());
    if !state.limiter.allow(&format!("ip:{ip_key}")) {
        return Err(ApiError::RateLimited);
    }
    let mut tx = state.pool.begin().await?;
    let row: Option<RefreshRow> = sqlx::query_as(
        "select id, user_id, family_id, expires_at, revoked_at from refresh_tokens where token_hash = $1 for update",
    )
    .bind(tokens::hash_refresh(&body.refresh_token))
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        return Err(ApiError::Unauthorized("invalid refresh token".to_string()));
    };
    let ctx = AuditCtx {
        user_id: Some(row.user_id),
        employee_id: None,
        ip: ip.clone(),
        request_id: current_request_id(),
    };
    if row.revoked_at.is_some() {
        // A replayed token means the family has leaked: revoke every token in it.
        sqlx::query("update refresh_tokens set revoked_at = now() where family_id = $1 and revoked_at is null")
            .bind(row.family_id)
            .execute(&mut *tx)
            .await?;
        audit::record(
            &mut tx,
            &ctx,
            "auth.refresh_reuse",
            "user",
            Some(row.user_id),
            None,
            Some(serde_json::json!({"family_id": row.family_id})),
        )
        .await?;
        tx.commit().await?;
        return Err(ApiError::Unauthorized(
            "refresh token reuse detected; session revoked".to_string(),
        ));
    }
    if row.expires_at <= Utc::now() {
        return Err(ApiError::Unauthorized("refresh token expired".to_string()));
    }
    let Some(user) = load_user_by_id(&mut tx, row.user_id).await? else {
        return Err(ApiError::Unauthorized("unknown user".to_string()));
    };
    ensure_usable(&user)?;
    let (response, new_id) = issue_session(
        &mut tx,
        &state,
        user.id,
        user.token_version,
        row.family_id,
        ip.as_deref(),
        user.must_change_password,
    )
    .await?;
    sqlx::query("update refresh_tokens set revoked_at = now(), replaced_by = $2 where id = $1")
        .bind(row.id)
        .bind(new_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(response))
}

#[utoipa::path(post, path = "/api/v1/auth/logout", tag = "auth", request_body = RefreshRequest,
    responses((status = 204)))]
pub async fn logout(
    State(state): State<AppState>,
    ValidatedJson(body): ValidatedJson<RefreshRequest>,
) -> ApiResult<StatusCode> {
    sqlx::query(
        "update refresh_tokens set revoked_at = now()
          where family_id = (select family_id from refresh_tokens where token_hash = $1)
            and revoked_at is null",
    )
    .bind(tokens::hash_refresh(&body.refresh_token))
    .execute(&state.pool)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/api/v1/auth/change-password", tag = "auth", request_body = ChangePasswordRequest,
    security(("bearer" = [])), responses((status = 204), (status = 401, body = crate::error::Problem)))]
pub async fn change_password(
    State(state): State<AppState>,
    actor: Actor,
    ValidatedJson(body): ValidatedJson<ChangePasswordRequest>,
) -> ApiResult<StatusCode> {
    password::check_strength(&body.new_password)?;
    if body.new_password == body.current_password {
        return Err(ApiError::validation(
            "new_password",
            "must differ from the current password",
        ));
    }
    let mut tx = state.pool.begin().await?;
    let Some(user) = load_user_by_id(&mut tx, actor.user_id()).await? else {
        return Err(ApiError::Unauthorized("unknown user".to_string()));
    };
    if !password::verify_async(body.current_password.clone(), user.password_hash.clone()).await? {
        return Err(ApiError::Unauthorized(
            "current password is incorrect".to_string(),
        ));
    }
    let hash = password::hash_async(body.new_password.clone()).await?;
    sqlx::query(
        "update users set password_hash = $2, must_change_password = false,
                token_version = token_version + 1, failed_logins = 0, locked_until = null
          where id = $1",
    )
    .bind(user.id)
    .bind(hash)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "update refresh_tokens set revoked_at = now() where user_id = $1 and revoked_at is null",
    )
    .bind(user.id)
    .execute(&mut *tx)
    .await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "user.change_password",
        "user",
        Some(user.id),
        None,
        None,
    )
    .await?;
    tx.commit().await?;
    state.principals.evict(user.id).await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(sqlx::FromRow)]
struct MeRow {
    employee_no: String,
    email: String,
    department_name: String,
    site: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/auth/me", tag = "auth", security(("bearer" = [])),
    responses((status = 200, body = MeResponse)))]
pub async fn me(State(state): State<AppState>, actor: Actor) -> ApiResult<Json<MeResponse>> {
    let p = &actor.principal;
    let row: MeRow = sqlx::query_as(
        "select e.employee_no, e.email::text as email, d.name as department_name, e.site
           from employees e join departments d on d.id = e.department_id where e.id = $1",
    )
    .bind(p.employee_id)
    .fetch_one(&state.pool)
    .await?;
    let mut conn = state.pool.acquire().await?;
    let chain = org::chain_of_command(&mut conn, &p.path, p.employee_id).await?;
    let mut permissions: Vec<String> = p.permissions.iter().cloned().collect();
    permissions.sort();
    Ok(Json(MeResponse {
        user: MeUser {
            id: p.user_id,
            email: p.email.clone(),
            status: p.user_status.clone(),
            must_change_password: p.must_change_password,
        },
        employee: MeEmployee {
            id: p.employee_id,
            employee_no: row.employee_no,
            first_name: p.first_name.clone(),
            last_name: p.last_name.clone(),
            email: row.email,
            title: p.title.clone(),
            level: p.level,
            position_id: p.position_id,
            department_id: p.department_id,
            department_name: row.department_name,
            manager_id: p.manager_id,
            site: row.site,
            status: p.employee_status.clone(),
        },
        roles: p.roles.clone(),
        permissions,
        chain,
    }))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/refresh", post(refresh))
        .route("/auth/logout", post(logout))
        .route("/auth/change-password", post(change_password))
        .route("/auth/me", get(me))
}

#[derive(OpenApi)]
#[openapi(paths(login, refresh, logout, change_password, me))]
pub struct AuthApi;
