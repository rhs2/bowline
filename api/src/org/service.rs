//! Organisation queries shared by several modules: employee lookups, visibility
//! checks, chain of command, reference numbers.

use chrono::Datelike;
use serde::Serialize;
use sqlx::PgConnection;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::error::{ApiError, ApiResult};
use crate::scope::ScopeFilter;

/// The handful of employee fields every authorisation decision needs.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct EmployeeCore {
    pub id: Uuid,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub manager_id: Option<Uuid>,
    pub department_id: Uuid,
    pub position_id: Uuid,
    pub level: i16,
    pub path: String,
    pub status: String,
}

impl EmployeeCore {
    pub fn full_name(&self) -> String {
        format!("{} {}", self.first_name, self.last_name)
    }
}

pub async fn load_core(conn: &mut PgConnection, id: Uuid) -> sqlx::Result<Option<EmployeeCore>> {
    sqlx::query_as(
        "select e.id, e.first_name, e.last_name, e.email::text as email, e.manager_id, e.department_id,
                e.position_id, p.level, e.path::text as path, e.status
           from employees e join positions p on p.id = e.position_id where e.id = $1",
    )
    .bind(id)
    .fetch_optional(conn)
    .await
}

/// Loads an employee and hides it (404) when outside the caller's scope.
pub async fn load_in_scope(
    conn: &mut PgConnection,
    filter: &ScopeFilter,
    id: Uuid,
) -> ApiResult<EmployeeCore> {
    match load_core(conn, id).await? {
        Some(e) if filter.contains(e.id, &e.path, e.department_id) => Ok(e),
        _ => Err(ApiError::not_found("employee")),
    }
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct ChainEntry {
    pub id: Uuid,
    pub name: String,
    pub title: String,
    pub level: i16,
}

/// Turns an ltree label back into the uuid it encodes.
pub fn label_to_uuid(label: &str) -> Option<Uuid> {
    Uuid::parse_str(&label.replace('_', "-")).ok()
}

/// Managers above an employee, nearest first, ending with the CEO.
pub async fn chain_of_command(
    conn: &mut PgConnection,
    path: &str,
    employee_id: Uuid,
) -> sqlx::Result<Vec<ChainEntry>> {
    let ids: Vec<Uuid> = path
        .split('.')
        .filter_map(label_to_uuid)
        .filter(|id| *id != employee_id)
        .collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<ChainEntry> = sqlx::query_as(
        "select e.id, e.first_name || ' ' || e.last_name as name, p.title, p.level
           from employees e join positions p on p.id = e.position_id where e.id = any($1)",
    )
    .bind(&ids)
    .fetch_all(conn)
    .await?;
    let mut ordered: Vec<ChainEntry> = Vec::with_capacity(rows.len());
    for id in ids.iter().rev() {
        if let Some(pos) = rows.iter().position(|r| r.id == *id) {
            let row = &rows[pos];
            ordered.push(ChainEntry {
                id: row.id,
                name: row.name.clone(),
                title: row.title.clone(),
                level: row.level,
            });
        }
    }
    Ok(ordered)
}

pub async fn next_employee_no(conn: &mut PgConnection) -> sqlx::Result<String> {
    let next: i64 = sqlx::query_scalar(
        "select coalesce(max(substring(employee_no from 5)::bigint), 0) + 1
           from employees where employee_no ~ '^EMP-[0-9]+$'",
    )
    .fetch_one(conn)
    .await?;
    Ok(format!("EMP-{next:06}"))
}

/// `BWL-YYYY-NNNNNN` from the shipment sequence.
pub async fn next_shipment_ref(conn: &mut PgConnection) -> sqlx::Result<String> {
    let n: i64 = sqlx::query_scalar("select nextval('shipment_ref_seq')")
        .fetch_one(conn)
        .await?;
    Ok(format!("BWL-{}-{n:06}", chrono::Utc::now().year()))
}

pub async fn next_invoice_no(conn: &mut PgConnection) -> sqlx::Result<String> {
    let n: i64 = sqlx::query_scalar("select nextval('invoice_no_seq')")
        .fetch_one(conn)
        .await?;
    Ok(format!("INV-{}-{n:06}", chrono::Utc::now().year()))
}

pub async fn next_ticket_no(conn: &mut PgConnection) -> sqlx::Result<String> {
    let n: i64 = sqlx::query_scalar("select nextval('ticket_no_seq')")
        .fetch_one(conn)
        .await?;
    Ok(format!("TKT-{n:06}"))
}

/// Default role for a position level when none is given explicitly.
pub fn role_for_level(level: i16) -> &'static str {
    match level {
        1 | 2 => "executive",
        3 => "director",
        4 => "manager",
        5 => "supervisor",
        6 => "staff",
        _ => "field_worker",
    }
}

/// Replaces a user's roles; `baseline` is always kept.
pub async fn set_roles(
    conn: &mut PgConnection,
    user_id: Uuid,
    granted_by: Option<Uuid>,
    roles: &[String],
) -> ApiResult<()> {
    let mut wanted: Vec<String> = roles.iter().map(|r| r.to_lowercase()).collect();
    if !wanted.iter().any(|r| r == "baseline") {
        wanted.push("baseline".to_string());
    }
    wanted.sort();
    wanted.dedup();
    let known: Vec<String> =
        sqlx::query_scalar("select key::text from roles where key = any($1::citext[])")
            .bind(&wanted)
            .fetch_all(&mut *conn)
            .await?;
    if let Some(unknown) = wanted
        .iter()
        .find(|r| !known.iter().any(|k| k.eq_ignore_ascii_case(r)))
    {
        return Err(ApiError::validation(
            "roles",
            format!("unknown role {unknown}"),
        ));
    }
    sqlx::query("delete from user_roles where user_id = $1")
        .bind(user_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query(
        "insert into user_roles (user_id, role_id, granted_by)
         select $1, r.id, $2 from roles r where r.key = any($3::citext[])",
    )
    .bind(user_id)
    .bind(granted_by)
    .bind(&wanted)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Creates the current year's leave balances for an employee from the type quotas.
pub async fn init_leave_balances(conn: &mut PgConnection, employee_id: Uuid) -> sqlx::Result<()> {
    let year = chrono::Utc::now().year() as i16;
    sqlx::query(
        "insert into leave_balances (employee_id, year, type_key, allocated, used)
         select $1, $2, key, annual_quota_days, 0 from leave_types
         on conflict do nothing",
    )
    .bind(employee_id)
    .bind(year)
    .execute(conn)
    .await?;
    Ok(())
}
