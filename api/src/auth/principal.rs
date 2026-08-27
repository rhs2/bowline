//! The principal: user, employee, roles, permission set and reporting path, loaded
//! once per request from a 60 second cache (Redis when available, memory otherwise).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::scope::Scope;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Principal {
    pub user_id: Uuid,
    pub employee_id: Uuid,
    pub email: String,
    pub user_status: String,
    pub token_version: i32,
    pub must_change_password: bool,
    pub first_name: String,
    pub last_name: String,
    pub title: String,
    pub level: i16,
    pub position_id: Uuid,
    pub department_id: Uuid,
    /// The principal's department and every department below it.
    pub department_ids: Vec<Uuid>,
    pub manager_id: Option<Uuid>,
    pub path: String,
    pub employee_status: String,
    pub roles: Vec<String>,
    pub permissions: HashSet<String>,
}

impl Principal {
    pub fn has(&self, permission: &str) -> bool {
        self.permissions.contains(permission)
    }

    pub fn has_any(&self, permissions: &[&str]) -> bool {
        permissions.iter().any(|p| self.has(p))
    }

    pub fn require(&self, permission: &str) -> ApiResult<()> {
        if self.has(permission) {
            Ok(())
        } else {
            Err(ApiError::Forbidden(format!("requires {permission}")))
        }
    }

    pub fn require_any(&self, permissions: &[&str]) -> ApiResult<()> {
        if self.has_any(permissions) {
            Ok(())
        } else {
            Err(ApiError::Forbidden(format!(
                "requires one of {}",
                permissions.join(", ")
            )))
        }
    }

    /// Widest scope held for a permission family such as `employees:read`.
    pub fn scope(&self, family: &str) -> Option<Scope> {
        if self.has(&format!("{family}:all")) {
            Some(Scope::All)
        } else if self.has(&format!("{family}:department")) {
            Some(Scope::Department)
        } else if self.has(&format!("{family}:subtree")) {
            Some(Scope::Subtree)
        } else if self.has(&format!("{family}:self")) {
            Some(Scope::Own)
        } else {
            None
        }
    }

    pub fn full_name(&self) -> String {
        format!("{} {}", self.first_name, self.last_name)
    }

    /// True when `path` (ltree text) is the principal's own path or below it.
    pub fn is_in_subtree(&self, path: &str) -> bool {
        path == self.path || path.starts_with(&format!("{}.", self.path))
    }

    pub fn is_strictly_below(&self, path: &str) -> bool {
        path != self.path && self.is_in_subtree(path)
    }
}

#[derive(sqlx::FromRow)]
struct PrincipalRow {
    user_id: Uuid,
    employee_id: Uuid,
    email: String,
    user_status: String,
    token_version: i32,
    must_change_password: bool,
    first_name: String,
    last_name: String,
    title: String,
    level: i16,
    position_id: Uuid,
    department_id: Uuid,
    manager_id: Option<Uuid>,
    path: String,
    employee_status: String,
}

pub async fn load(pool: &PgPool, user_id: Uuid) -> Result<Option<Principal>, sqlx::Error> {
    let row: Option<PrincipalRow> = sqlx::query_as(
        "select u.id as user_id, e.id as employee_id, u.email::text as email, u.status as user_status,
                u.token_version, u.must_change_password, e.first_name, e.last_name, p.title, p.level,
                e.position_id, e.department_id, e.manager_id, e.path::text as path,
                e.status as employee_status
           from users u
           join employees e on e.id = u.employee_id
           join positions p on p.id = e.position_id
          where u.id = $1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let roles: Vec<String> = sqlx::query_scalar(
        "select r.key::text from user_roles ur join roles r on r.id = ur.role_id
          where ur.user_id = $1 order by r.key",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    let permissions: Vec<String> =
        sqlx::query_scalar("select permission_key::text from user_permissions where user_id = $1")
            .bind(user_id)
            .fetch_all(pool)
            .await?;
    let department_ids: Vec<Uuid> = department_subtree(pool, row.department_id).await?;
    Ok(Some(Principal {
        user_id: row.user_id,
        employee_id: row.employee_id,
        email: row.email,
        user_status: row.user_status,
        token_version: row.token_version,
        must_change_password: row.must_change_password,
        first_name: row.first_name,
        last_name: row.last_name,
        title: row.title,
        level: row.level,
        position_id: row.position_id,
        department_id: row.department_id,
        department_ids,
        manager_id: row.manager_id,
        path: row.path,
        employee_status: row.employee_status,
        roles,
        permissions: permissions.into_iter().collect(),
    }))
}

pub async fn department_subtree<'e, E>(executor: E, root: Uuid) -> Result<Vec<Uuid>, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query_scalar(
        "with recursive d as (
            select id from departments where id = $1
            union all
            select c.id from departments c join d on c.parent_id = d.id)
         select id from d",
    )
    .bind(root)
    .fetch_all(executor)
    .await
}

const TTL: Duration = Duration::from_secs(60);

/// Cached principals by user id, each with the instant it was stored.
type LocalCache = Arc<RwLock<HashMap<Uuid, (Instant, Arc<Principal>)>>>;

#[derive(Clone)]
pub struct PrincipalCache {
    local: LocalCache,
    redis: Option<redis::aio::ConnectionManager>,
}

impl PrincipalCache {
    pub fn new(redis: Option<redis::aio::ConnectionManager>) -> Self {
        Self {
            local: Arc::new(RwLock::new(HashMap::new())),
            redis,
        }
    }

    fn key(user_id: Uuid) -> String {
        format!("bowline:principal:{user_id}")
    }

    pub async fn get(&self, user_id: Uuid) -> Option<Arc<Principal>> {
        {
            let local = self.local.read().await;
            if let Some((stored, p)) = local.get(&user_id) {
                if stored.elapsed() < TTL {
                    return Some(p.clone());
                }
            }
        }
        if let Some(redis) = &self.redis {
            let mut conn = redis.clone();
            let cached: Result<Option<String>, _> = conn.get(Self::key(user_id)).await;
            if let Ok(Some(json)) = cached {
                if let Ok(p) = serde_json::from_str::<Principal>(&json) {
                    let p = Arc::new(p);
                    self.local
                        .write()
                        .await
                        .insert(user_id, (Instant::now(), p.clone()));
                    return Some(p);
                }
            }
        }
        None
    }

    pub async fn put(&self, principal: Arc<Principal>) {
        self.local
            .write()
            .await
            .insert(principal.user_id, (Instant::now(), principal.clone()));
        if let Some(redis) = &self.redis {
            if let Ok(json) = serde_json::to_string(principal.as_ref()) {
                let mut conn = redis.clone();
                let res: Result<(), _> = conn
                    .set_ex(Self::key(principal.user_id), json, TTL.as_secs())
                    .await;
                if let Err(e) = res {
                    tracing::debug!(error = %e, "redis principal cache write failed");
                }
            }
        }
    }

    pub async fn evict(&self, user_id: Uuid) {
        self.local.write().await.remove(&user_id);
        if let Some(redis) = &self.redis {
            let mut conn = redis.clone();
            let res: Result<(), _> = conn.del(Self::key(user_id)).await;
            if let Err(e) = res {
                tracing::debug!(error = %e, "redis principal cache delete failed");
            }
        }
    }
}

/// Loads the principal for a validated token, refusing tokens whose version no
/// longer matches the user (password change, role change, reset).
pub async fn resolve(
    pool: &PgPool,
    cache: &PrincipalCache,
    user_id: Uuid,
    token_version: i32,
) -> ApiResult<Arc<Principal>> {
    if let Some(p) = cache.get(user_id).await {
        if p.token_version == token_version {
            return Ok(p);
        }
        cache.evict(user_id).await;
    }
    let principal = load(pool, user_id)
        .await?
        .ok_or_else(|| ApiError::Unauthorized("unknown user".to_string()))?;
    if principal.token_version != token_version {
        return Err(ApiError::Unauthorized(
            "access token has been revoked".to_string(),
        ));
    }
    let principal = Arc::new(principal);
    cache.put(principal.clone()).await;
    Ok(principal)
}
