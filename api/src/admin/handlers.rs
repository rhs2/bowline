//! Platform administration: user accounts, role assignment and the audit trail.

use axum::extract::{Path, Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgConnection, Postgres, QueryBuilder};
use utoipa::{IntoParams, OpenApi, ToSchema};
use uuid::Uuid;
use validator::Validate;

use crate::audit;
use crate::auth::password;
use crate::auth::Actor;
use crate::error::{ApiError, ApiResult, Problem};
use crate::http::extract::ValidatedJson;
use crate::http::pagination::{split_total, PageOut, PageQuery};
use crate::org::service as org;
use crate::state::AppState;

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct UserOut {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub employee_no: String,
    pub name: String,
    pub email: String,
    pub title: String,
    pub department_name: String,
    pub status: String,
    pub employee_status: String,
    pub must_change_password: bool,
    pub failed_logins: i32,
    pub locked_until: Option<DateTime<Utc>>,
    pub last_login_at: Option<DateTime<Utc>>,
    pub roles: Vec<String>,
    pub created_at: DateTime<Utc>,
}

const USER_SELECT: &str = "select u.id, u.employee_id, e.employee_no,
                e.first_name || ' ' || e.last_name as name, u.email::text as email, p.title,
                d.name as department_name, u.status, e.status as employee_status,
                u.must_change_password, u.failed_logins, u.locked_until, u.last_login_at,
                coalesce(array_agg(r.key::text order by r.key) filter (where r.id is not null),
                         '{}'::text[]) as roles,
                u.created_at, count(*) over() as total_count
           from users u
           join employees e on e.id = u.employee_id
           join positions p on p.id = e.position_id
           join departments d on d.id = e.department_id
           left join user_roles ur on ur.user_id = u.id
           left join roles r on r.id = ur.role_id
          where ";

const USER_GROUP_BY: &str =
    " group by u.id, e.id, e.employee_no, e.first_name, e.last_name, p.title, d.name ";

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct UserFilter {
    /// `active`, `locked` or `disabled`.
    pub status: Option<String>,
    /// Only users holding this role key.
    pub role: Option<String>,
    /// `1` returns only accounts that still owe a password change.
    pub must_change_password: Option<u8>,
}

#[utoipa::path(get, path = "/api/v1/admin/users", tag = "admin", security(("bearer" = [])),
    params(PageQuery, UserFilter),
    responses((status = 200, body = PageOut<UserOut>), (status = 403, body = Problem)))]
pub async fn list_users(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<UserFilter>,
) -> ApiResult<Json<PageOut<UserOut>>> {
    actor.require("users:manage")?;
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(USER_SELECT);
    qb.push(" true ");
    if let Some(status) = &filter.status {
        qb.push(" and u.status = ").push_bind(status.clone());
    }
    if filter.must_change_password == Some(1) {
        qb.push(" and u.must_change_password ");
    }
    if let Some(role) = &filter.role {
        qb.push(
            " and exists (select 1 from user_roles ur2 join roles r2 on r2.id = ur2.role_id
                           where ur2.user_id = u.id and r2.key = ",
        )
        .push_bind(role.clone())
        .push("::citext) ");
    }
    if let Some(q) = page.search() {
        qb.push(" and (e.first_name || ' ' || e.last_name ilike ")
            .push_bind(q.clone())
            .push(" or u.email::text ilike ")
            .push_bind(q.clone())
            .push(" or e.employee_no ilike ")
            .push_bind(q)
            .push(")");
    }
    qb.push(USER_GROUP_BY);
    let order = page.order_by(&[
        ("name", "e.last_name asc, e.first_name"),
        ("email", "u.email"),
        ("last_login", "u.last_login_at"),
        ("status", "u.status"),
        ("created_at", "u.created_at"),
    ]);
    let order = if page.sort.is_none() {
        "e.last_name asc, e.first_name asc".to_string()
    } else {
        order
    };
    qb.push(format!(" order by {order} limit "));
    let paging = page.page();
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&state.pool).await?;
    let (items, total) = split_total::<UserOut>(rows)?;
    Ok(Json(PageOut::new(items, paging, total)))
}

async fn load_user(conn: &mut PgConnection, id: Uuid) -> ApiResult<UserOut> {
    let sql = format!("{USER_SELECT} u.id = $1 {USER_GROUP_BY}");
    sqlx::query_as(&sql)
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(|| ApiError::not_found("user"))
}

async fn lock_user_row(conn: &mut PgConnection, id: Uuid) -> ApiResult<String> {
    sqlx::query_scalar("select status from users where id = $1 for update")
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(|| ApiError::not_found("user"))
}

async fn revoke_sessions(conn: &mut PgConnection, id: Uuid) -> ApiResult<u64> {
    let revoked = sqlx::query(
        "update refresh_tokens set revoked_at = now() where user_id = $1 and revoked_at is null",
    )
    .bind(id)
    .execute(conn)
    .await?
    .rows_affected();
    Ok(revoked)
}

#[utoipa::path(post, path = "/api/v1/admin/users/{id}/lock", tag = "admin", security(("bearer" = [])),
    responses((status = 200, body = UserOut), (status = 403, body = Problem),
              (status = 409, body = Problem)))]
pub async fn lock_user(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<UserOut>> {
    actor.require("users:manage")?;
    if id == actor.user_id() {
        return Err(ApiError::forbidden("you cannot lock your own account"));
    }
    let mut tx = state.pool.begin().await?;
    let status = lock_user_row(&mut tx, id).await?;
    if status != "active" {
        return Err(ApiError::transition(&status, "locked"));
    }
    let before = audit::snapshot(&mut tx, "users", id).await?;
    // Locking bumps the token version, so access tokens already in flight stop working.
    sqlx::query(
        "update users set status = 'locked', token_version = token_version + 1 where id = $1",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    let revoked = revoke_sessions(&mut tx, id).await?;
    let after = audit::snapshot(&mut tx, "users", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "user.lock",
        "user",
        Some(id),
        before,
        after.map(|u| json!({"user": u, "sessions_revoked": revoked})),
    )
    .await?;
    let user = load_user(&mut tx, id).await?;
    tx.commit().await?;
    state.principals.evict(id).await;
    Ok(Json(user))
}

#[utoipa::path(post, path = "/api/v1/admin/users/{id}/unlock", tag = "admin", security(("bearer" = [])),
    responses((status = 200, body = UserOut), (status = 409, body = Problem)))]
pub async fn unlock_user(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<UserOut>> {
    actor.require("users:manage")?;
    let mut tx = state.pool.begin().await?;
    let status = lock_user_row(&mut tx, id).await?;
    if status != "locked" {
        return Err(ApiError::transition(&status, "active"));
    }
    let before = audit::snapshot(&mut tx, "users", id).await?;
    sqlx::query(
        "update users set status = 'active', failed_logins = 0, locked_until = null where id = $1",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "users", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "user.unlock",
        "user",
        Some(id),
        before,
        after,
    )
    .await?;
    let user = load_user(&mut tx, id).await?;
    tx.commit().await?;
    state.principals.evict(id).await;
    Ok(Json(user))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TemporaryPassword {
    pub user: UserOut,
    /// Shown once; the user has to change it at the next login.
    pub temporary_password: String,
}

#[utoipa::path(post, path = "/api/v1/admin/users/{id}/reset-password", tag = "admin",
    security(("bearer" = [])),
    responses((status = 200, body = TemporaryPassword), (status = 409, body = Problem)))]
pub async fn reset_password(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TemporaryPassword>> {
    actor.require("users:manage")?;
    // Argon2id is deliberately slow, so hash before the transaction is opened.
    let temporary_password = password::generate_temporary();
    let hash = password::hash_async(temporary_password.clone()).await?;
    let mut tx = state.pool.begin().await?;
    let status = lock_user_row(&mut tx, id).await?;
    if status == "disabled" {
        return Err(ApiError::conflict(
            "this account is disabled; reinstate the employee before resetting the password",
        ));
    }
    let before = audit::snapshot(&mut tx, "users", id).await?;
    sqlx::query(
        "update users set password_hash = $2, must_change_password = true,
                token_version = token_version + 1, failed_logins = 0, locked_until = null,
                status = case when status = 'locked' then 'active' else status end
          where id = $1",
    )
    .bind(id)
    .bind(&hash)
    .execute(&mut *tx)
    .await?;
    let revoked = revoke_sessions(&mut tx, id).await?;
    let after = audit::snapshot(&mut tx, "users", id).await?;
    // The password itself never reaches the audit log; the hash in the snapshot is
    // dropped for the same reason.
    audit::record(
        &mut tx,
        &actor.audit(),
        "user.reset_password",
        "user",
        Some(id),
        before.map(redact_password),
        after.map(|u| {
            json!({"user": redact_password(u), "sessions_revoked": revoked, "must_change_password": true})
        }),
    )
    .await?;
    let user = load_user(&mut tx, id).await?;
    tx.commit().await?;
    state.principals.evict(id).await;
    Ok(Json(TemporaryPassword {
        user,
        temporary_password,
    }))
}

fn redact_password(mut snapshot: serde_json::Value) -> serde_json::Value {
    if let Some(object) = snapshot.as_object_mut() {
        object.remove("password_hash");
    }
    snapshot
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct SetRoles {
    /// Role keys; `baseline` is always kept.
    pub roles: Vec<String>,
}

#[utoipa::path(put, path = "/api/v1/admin/users/{id}/roles", tag = "admin", security(("bearer" = [])),
    request_body = SetRoles,
    responses((status = 200, body = UserOut), (status = 403, body = Problem),
              (status = 422, body = Problem)))]
pub async fn set_user_roles(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<SetRoles>,
) -> ApiResult<Json<UserOut>> {
    actor.require("roles:manage")?;
    if body.roles.len() > 20 {
        return Err(ApiError::validation("roles", "at most 20 roles"));
    }
    let mut tx = state.pool.begin().await?;
    let before = load_user(&mut tx, id).await?;
    org::set_roles(&mut tx, id, Some(actor.user_id()), &body.roles).await?;
    let user = load_user(&mut tx, id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "user.set_roles",
        "user",
        Some(id),
        Some(json!({"roles": before.roles})),
        Some(json!({"roles": user.roles})),
    )
    .await?;
    tx.commit().await?;
    // Permissions are cached for a minute; drop the entry so the change is immediate.
    state.principals.evict(id).await;
    Ok(Json(user))
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct RoleOut {
    pub id: i16,
    pub key: String,
    pub name: String,
    pub description: String,
    pub permissions: Vec<String>,
    pub user_count: i64,
}

#[utoipa::path(get, path = "/api/v1/admin/roles", tag = "admin", security(("bearer" = [])),
    params(PageQuery),
    responses((status = 200, body = PageOut<RoleOut>), (status = 403, body = Problem)))]
pub async fn list_roles(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
) -> ApiResult<Json<PageOut<RoleOut>>> {
    actor.require_any(&["roles:manage", "users:manage", "audit:read"])?;
    let paging = page.page();
    let rows = sqlx::query(
        "select r.id, r.key::text as key, r.name, r.description,
                coalesce(array_agg(rp.permission_key::text order by rp.permission_key)
                         filter (where rp.permission_key is not null), '{}'::text[]) as permissions,
                (select count(*) from user_roles ur where ur.role_id = r.id) as user_count,
                count(*) over() as total_count
           from roles r
           left join role_permissions rp on rp.role_id = r.id
          group by r.id, r.key, r.name, r.description
          order by r.key
          limit $1 offset $2",
    )
    .bind(paging.limit())
    .bind(paging.offset())
    .fetch_all(&state.pool)
    .await?;
    let (items, total) = split_total::<RoleOut>(rows)?;
    Ok(Json(PageOut::new(items, paging, total)))
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct AuditEntry {
    pub id: i64,
    pub at: DateTime<Utc>,
    pub actor_user_id: Option<Uuid>,
    pub actor_employee_id: Option<Uuid>,
    pub actor_name: Option<String>,
    pub action: String,
    pub entity_type: String,
    pub entity_id: Option<Uuid>,
    #[schema(value_type = Option<Object>)]
    pub before: Option<serde_json::Value>,
    #[schema(value_type = Option<Object>)]
    pub after: Option<serde_json::Value>,
    pub ip: Option<String>,
    pub request_id: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AuditFilter {
    pub entity_type: Option<String>,
    pub entity_id: Option<Uuid>,
    /// Employee id of the person who acted.
    pub actor: Option<Uuid>,
    pub action: Option<String>,
    /// Calendar day, inclusive. Every other date filter in the API takes a plain
    /// `YYYY-MM-DD`, which is also what a date input sends, so this one does too.
    pub from: Option<NaiveDate>,
    /// Calendar day, inclusive: the whole of this day is included, not the instant
    /// it begins. Without that, filtering `to` today returns nothing from today.
    pub to: Option<NaiveDate>,
}

#[utoipa::path(get, path = "/api/v1/admin/audit", tag = "admin", security(("bearer" = [])),
    params(PageQuery, AuditFilter),
    responses((status = 200, body = PageOut<AuditEntry>), (status = 403, body = Problem)))]
pub async fn list_audit(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<AuditFilter>,
) -> ApiResult<Json<PageOut<AuditEntry>>> {
    actor.require("audit:read")?;
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        "select a.id, a.at, a.actor_user_id, a.actor_employee_id,
                e.first_name || ' ' || e.last_name as actor_name, a.action, a.entity_type,
                a.entity_id, a.before, a.after, host(a.ip) as ip, a.request_id,
                count(*) over() as total_count
           from audit_log a
           left join employees e on e.id = a.actor_employee_id
          where true ",
    );
    if let Some(entity_type) = &filter.entity_type {
        qb.push(" and a.entity_type = ")
            .push_bind(entity_type.clone());
    }
    if let Some(entity_id) = filter.entity_id {
        qb.push(" and a.entity_id = ").push_bind(entity_id);
    }
    if let Some(who) = filter.actor {
        qb.push(" and a.actor_employee_id = ").push_bind(who);
    }
    if let Some(action) = &filter.action {
        qb.push(" and a.action = ").push_bind(action.clone());
    }
    if let Some(from) = filter.from {
        qb.push(" and a.at >= ")
            .push_bind(from.and_hms_opt(0, 0, 0).unwrap_or_default().and_utc());
    }
    if let Some(to) = filter.to {
        // The day after, exclusive, so the whole of `to` is covered whatever time
        // of day a row was written.
        let end = to
            .succ_opt()
            .unwrap_or(to)
            .and_hms_opt(0, 0, 0)
            .unwrap_or_default()
            .and_utc();
        qb.push(" and a.at < ").push_bind(end);
    }
    if let Some(q) = page.search() {
        qb.push(" and a.action ilike ").push_bind(q);
    }
    qb.push(" order by a.at desc, a.id desc limit ");
    let paging = page.page();
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&state.pool).await?;
    let (items, total) = split_total::<AuditEntry>(rows)?;
    Ok(Json(PageOut::new(items, paging, total)))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/admin/users", get(list_users))
        .route("/admin/users/:id/lock", post(lock_user))
        .route("/admin/users/:id/unlock", post(unlock_user))
        .route("/admin/users/:id/reset-password", post(reset_password))
        .route("/admin/users/:id/roles", put(set_user_roles))
        .route("/admin/roles", get(list_roles))
        .route("/admin/audit", get(list_audit))
}

#[derive(OpenApi)]
#[openapi(paths(
    list_users,
    lock_user,
    unlock_user,
    reset_password,
    set_user_roles,
    list_roles,
    list_audit
))]
pub struct AdminApi;
