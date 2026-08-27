//! HR helpers shared by the handlers: who may see whose records, the small leave
//! calculations, and the object keys behind employee documents.

use chrono::{Datelike, NaiveDate, Utc};
use rust_decimal::Decimal;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::auth::Actor;
use crate::clients::s3::sanitise_filename;
use crate::error::{ApiError, ApiResult};
use crate::scope::{Scope, ScopeFilter};

/// Document kinds accepted by `employee_documents`.
pub const DOCUMENT_KINDS: [&str; 5] = ["contract", "id", "certificate", "payslip", "other"];
/// Attendance sources accepted by `attendance`.
pub const ATTENDANCE_SOURCES: [&str; 4] = ["web", "mobile", "kiosk", "import"];
/// Leave request states.
pub const LEAVE_STATUSES: [&str; 4] = ["pending", "approved", "rejected", "cancelled"];
/// Grace period before a clock-in counts as late.
pub const LATE_AFTER_MINUTES: i64 = 10;
/// Upper bound for a presigned employee document.
pub const MAX_DOCUMENT_BYTES: i64 = 25 * 1024 * 1024;
/// Longest leave request the API accepts in one go.
pub const MAX_LEAVE_DAYS: i64 = 365;

pub fn current_year() -> i16 {
    Utc::now().year() as i16
}

/// Leave is booked against the year the absence starts in.
pub fn leave_year(start: NaiveDate) -> i16 {
    start.year() as i16
}

/// Query flags are written `?flag=1` in the API contract; `true`, `yes`, `on` and a
/// bare `?flag` mean the same thing.
pub fn truthy(value: Option<&str>) -> bool {
    matches!(value.map(str::trim), Some("1" | "true" | "yes" | "on" | ""))
}

/// Widest scope the caller holds over leave records: HR sees everyone, approvers
/// see their subtree, everybody else sees themselves.
pub fn leave_scope(actor: &Actor) -> Scope {
    if actor.has("leave:manage:all") {
        Scope::All
    } else if actor.has("leave:approve:subtree") {
        Scope::Subtree
    } else {
        Scope::Own
    }
}

pub fn leave_filter(actor: &Actor) -> ScopeFilter {
    actor.filter(leave_scope(actor))
}

/// Shifts and attendance: supervisors and above see the people who report up to
/// them, HR and executives see everyone.
pub fn roster_filter(actor: &Actor) -> ScopeFilter {
    let scope = if actor.has("employees:read:all") || actor.has("leave:manage:all") {
        Scope::All
    } else if actor.has("shifts:manage:subtree") || actor.has("employees:read:subtree") {
        Scope::Subtree
    } else {
        Scope::Own
    };
    actor.filter(scope)
}

/// Document *lists*: HR sees everyone, managers see their subtree, everybody else
/// sees themselves. Downloads are narrower, see [`may_download`].
pub fn document_list_filter(actor: &Actor) -> ScopeFilter {
    let scope = if actor.has("documents:manage:all") {
        Scope::All
    } else if actor.has("employees:read:subtree") {
        Scope::Subtree
    } else {
        Scope::Own
    };
    actor.filter(scope)
}

/// Files themselves are private to the employee and to HR; a manager may see that a
/// contract exists but never its contents.
pub fn may_download(actor: &Actor, employee_id: Uuid) -> bool {
    employee_id == actor.me() || actor.has("documents:manage:all")
}

/// True when the caller may decide on leave for the employee at `path`.
pub fn may_decide(actor: &Actor, employee_path: &str) -> bool {
    actor.has("leave:manage:all") || actor.principal.is_strictly_below(employee_path)
}

/// Leave spans whole days and counts both ends.
pub fn whole_days(start: NaiveDate, end: NaiveDate) -> i64 {
    (end - start).num_days() + 1
}

/// The exclusion constraint on `leave_requests` raises this when two pending or
/// approved requests for one employee overlap.
pub fn is_overlap(err: &sqlx::Error) -> bool {
    matches!(err, sqlx::Error::Database(db) if db.code().as_deref() == Some("23P01"))
}

/// Object key for an employee document. Keys are built from ids and a sanitised
/// title, never from raw input, and always carry the owning employee as a prefix.
pub fn document_key(employee_id: Uuid, kind: &str, title: &str) -> String {
    format!(
        "{}{}/{}-{}",
        document_prefix(employee_id),
        kind,
        Uuid::new_v4(),
        sanitise_filename(title)
    )
}

pub fn document_prefix(employee_id: Uuid) -> String {
    format!("employees/{employee_id}/")
}

pub fn check_one_of(field: &'static str, value: &str, allowed: &[&str]) -> ApiResult<()> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(ApiError::validation(
            field,
            format!("must be one of {}", allowed.join(", ")),
        ))
    }
}

/// The leave request fields every authorisation decision needs.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LeaveRequestCore {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub employee_name: String,
    pub employee_path: String,
    pub type_key: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub days: Decimal,
    pub status: String,
    pub current_approver_id: Option<Uuid>,
}

pub async fn load_request(conn: &mut PgConnection, id: Uuid) -> ApiResult<LeaveRequestCore> {
    sqlx::query_as(
        "select lr.id, lr.employee_id, e.first_name || ' ' || e.last_name as employee_name,
                e.path::text as employee_path, lr.type_key::text as type_key, lr.start_date,
                lr.end_date, lr.days, lr.status, lr.current_approver_id
           from leave_requests lr join employees e on e.id = lr.employee_id
          where lr.id = $1
          for update of lr",
    )
    .bind(id)
    .fetch_optional(conn)
    .await?
    .ok_or_else(|| ApiError::not_found("leave request"))
}

/// Allocation and usage for one leave type, falling back to the type quota when the
/// year's balance row has not been created yet. `None` means the type is unknown.
pub async fn balance_for(
    conn: &mut PgConnection,
    employee_id: Uuid,
    year: i16,
    type_key: &str,
) -> sqlx::Result<Option<(Decimal, Decimal)>> {
    let row: Option<(Decimal, Decimal)> = sqlx::query_as(
        "select coalesce(b.allocated, lt.annual_quota_days) as allocated, coalesce(b.used, 0) as used
           from leave_types lt
           left join leave_balances b
             on b.type_key = lt.key and b.employee_id = $1 and b.year = $2
          where lt.key = $3::citext",
    )
    .bind(employee_id)
    .bind(year)
    .bind(type_key)
    .fetch_optional(conn)
    .await?;
    Ok(row)
}

/// Moves `days` on or off a balance, creating the year's row from the type quota
/// when it does not exist yet. Negative values give days back after a cancellation.
pub async fn apply_balance(
    conn: &mut PgConnection,
    employee_id: Uuid,
    year: i16,
    type_key: &str,
    days: Decimal,
) -> sqlx::Result<()> {
    sqlx::query(
        "insert into leave_balances (employee_id, year, type_key, allocated, used)
         select $1, $2, lt.key, lt.annual_quota_days, greatest($4, 0)
           from leave_types lt where lt.key = $3::citext
         on conflict (employee_id, year, type_key)
         do update set used = greatest(leave_balances.used + $4, 0)",
    )
    .bind(employee_id)
    .bind(year)
    .bind(type_key)
    .bind(days)
    .execute(conn)
    .await?;
    Ok(())
}
