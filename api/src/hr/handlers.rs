//! HR endpoints: leave types, balances and requests, shifts, attendance and
//! employee documents.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, Postgres, QueryBuilder};
use utoipa::{IntoParams, OpenApi, ToSchema};
use uuid::Uuid;
use validator::Validate;

use crate::audit;
use crate::auth::Actor;
use crate::error::{ApiError, ApiResult, Problem};
use crate::hr::service;
use crate::http::extract::ValidatedJson;
use crate::http::pagination::{split_total, PageOut, PageQuery};
use crate::org::service as org;
use crate::outbox;
use crate::scope::Scope;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Leave types and balances
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct LeaveTypeOut {
    pub key: String,
    pub name: String,
    pub paid: bool,
    pub annual_quota_days: Decimal,
}

#[utoipa::path(get, path = "/api/v1/hr/leave/types", tag = "hr", security(("bearer" = [])),
    responses((status = 200, body = Vec<LeaveTypeOut>)))]
pub async fn leave_types(
    State(state): State<AppState>,
    _actor: Actor,
) -> ApiResult<Json<Vec<LeaveTypeOut>>> {
    let rows: Vec<LeaveTypeOut> = sqlx::query_as(
        "select key::text as key, name, paid, annual_quota_days from leave_types order by name",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct LeaveBalanceOut {
    pub employee_id: Uuid,
    pub employee_name: String,
    pub year: i16,
    pub type_key: String,
    pub type_name: String,
    pub allocated: Decimal,
    pub used: Decimal,
    pub remaining: Decimal,
}

const BALANCE_SELECT: &str =
    "select b.employee_id, e.first_name || ' ' || e.last_name as employee_name, b.year,
            b.type_key::text as type_key, lt.name as type_name, b.allocated, b.used,
            (b.allocated - b.used) as remaining, count(*) over() as total_count
       from leave_balances b
       join employees e on e.id = b.employee_id
       join leave_types lt on lt.key = b.type_key
      where ";

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct BalanceFilter {
    /// Defaults to everyone the caller may see.
    pub employee_id: Option<Uuid>,
    /// Defaults to the current calendar year.
    pub year: Option<i16>,
}

#[utoipa::path(get, path = "/api/v1/hr/leave/balances", tag = "hr", security(("bearer" = [])),
    params(PageQuery, BalanceFilter),
    responses((status = 200, body = PageOut<LeaveBalanceOut>), (status = 404, body = Problem)))]
pub async fn leave_balances(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<BalanceFilter>,
) -> ApiResult<Json<PageOut<LeaveBalanceOut>>> {
    let scope = service::leave_filter(&actor);
    let mut conn = state.pool.acquire().await?;
    if let Some(employee_id) = filter.employee_id {
        org::load_in_scope(&mut conn, &scope, employee_id).await?;
    }
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(BALANCE_SELECT);
    scope.push(&mut qb, "e");
    qb.push(" and b.year = ")
        .push_bind(filter.year.unwrap_or_else(service::current_year));
    if let Some(employee_id) = filter.employee_id {
        qb.push(" and b.employee_id = ").push_bind(employee_id);
    }
    let paging = page.page();
    qb.push(" order by e.last_name, e.first_name, b.type_key limit ");
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&mut *conn).await?;
    let (items, total) = split_total::<LeaveBalanceOut>(rows)?;
    Ok(Json(PageOut::new(items, paging, total)))
}

// ---------------------------------------------------------------------------
// Leave requests
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct LeaveRequestOut {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub employee_name: String,
    pub type_key: String,
    pub type_name: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub days: Decimal,
    pub reason: Option<String>,
    pub status: String,
    pub current_approver_id: Option<Uuid>,
    pub current_approver_name: Option<String>,
    pub decided_by: Option<Uuid>,
    pub decided_at: Option<DateTime<Utc>>,
    pub decision_note: Option<String>,
    pub created_at: DateTime<Utc>,
}

const LEAVE_SELECT: &str =
    "select lr.id, lr.employee_id, e.first_name || ' ' || e.last_name as employee_name,
            lr.type_key::text as type_key, lt.name as type_name, lr.start_date, lr.end_date,
            lr.days, lr.reason, lr.status, lr.current_approver_id,
            a.first_name || ' ' || a.last_name as current_approver_name,
            lr.decided_by, lr.decided_at, lr.decision_note, lr.created_at,
            count(*) over() as total_count
       from leave_requests lr
       join employees e on e.id = lr.employee_id
       join leave_types lt on lt.key = lr.type_key
       left join employees a on a.id = lr.current_approver_id
      where ";

async fn fetch_leave_request(conn: &mut PgConnection, id: Uuid) -> ApiResult<LeaveRequestOut> {
    let row: LeaveRequestOut = sqlx::query_as(&format!("{LEAVE_SELECT} lr.id = $1"))
        .bind(id)
        .fetch_one(conn)
        .await?;
    Ok(row)
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LeaveRequestFilter {
    pub employee_id: Option<Uuid>,
    /// `pending`, `approved`, `rejected` or `cancelled`.
    pub status: Option<String>,
    /// `1` to return only the requests waiting for the caller's decision.
    pub pending_for_me: Option<String>,
    /// Requests overlapping this window.
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

#[utoipa::path(get, path = "/api/v1/hr/leave/requests", tag = "hr", security(("bearer" = [])),
    params(PageQuery, LeaveRequestFilter),
    responses((status = 200, body = PageOut<LeaveRequestOut>)))]
pub async fn list_leave_requests(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<LeaveRequestFilter>,
) -> ApiResult<Json<PageOut<LeaveRequestOut>>> {
    let pending_for_me = service::truthy(filter.pending_for_me.as_deref());
    let scope = service::leave_filter(&actor);
    let mut conn = state.pool.acquire().await?;
    if let Some(employee_id) = filter.employee_id {
        org::load_in_scope(&mut conn, &scope, employee_id).await?;
    }
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(LEAVE_SELECT);
    if pending_for_me {
        // Approvers see what was routed to them even when it sits outside the
        // subtree they can otherwise browse.
        qb.push(" lr.current_approver_id = ")
            .push_bind(actor.me())
            .push(" and lr.status = 'pending'");
    } else {
        scope.push(&mut qb, "e");
    }
    if let Some(employee_id) = filter.employee_id {
        qb.push(" and lr.employee_id = ").push_bind(employee_id);
    }
    if let Some(status) = &filter.status {
        service::check_one_of("status", status, &service::LEAVE_STATUSES)?;
        qb.push(" and lr.status = ").push_bind(status.clone());
    }
    if let Some(from) = filter.from {
        qb.push(" and lr.end_date >= ").push_bind(from);
    }
    if let Some(to) = filter.to {
        qb.push(" and lr.start_date <= ").push_bind(to);
    }
    let order = page.order_by(&[
        ("start_date", "lr.start_date"),
        ("created_at", "lr.created_at"),
        ("status", "lr.status"),
        ("days", "lr.days"),
    ]);
    let paging = page.page();
    qb.push(format!(" order by {order} limit "));
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&mut *conn).await?;
    let (items, total) = split_total::<LeaveRequestOut>(rows)?;
    Ok(Json(PageOut::new(items, paging, total)))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateLeaveRequest {
    /// HR may file on someone else's behalf; defaults to the caller.
    pub employee_id: Option<Uuid>,
    #[validate(length(min = 1, max = 32))]
    pub type_key: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    #[validate(length(max = 2000))]
    pub reason: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/hr/leave/requests", tag = "hr", security(("bearer" = [])),
    request_body = CreateLeaveRequest,
    responses((status = 201, body = LeaveRequestOut), (status = 409, body = Problem)))]
pub async fn create_leave_request(
    State(state): State<AppState>,
    actor: Actor,
    ValidatedJson(body): ValidatedJson<CreateLeaveRequest>,
) -> ApiResult<(StatusCode, Json<LeaveRequestOut>)> {
    let employee_id = body.employee_id.unwrap_or_else(|| actor.me());
    if employee_id == actor.me() {
        actor.require("leave:request")?;
    } else {
        actor.require("leave:manage:all")?;
    }
    if body.end_date < body.start_date {
        return Err(ApiError::validation(
            "end_date",
            "must not be before start_date",
        ));
    }
    let days = service::whole_days(body.start_date, body.end_date);
    if days > service::MAX_LEAVE_DAYS {
        return Err(ApiError::validation(
            "end_date",
            format!("a request may not exceed {} days", service::MAX_LEAVE_DAYS),
        ));
    }
    let days = Decimal::from(days);
    let mut tx = state.pool.begin().await?;
    let employee = org::load_core(&mut tx, employee_id)
        .await?
        .ok_or_else(|| ApiError::validation("employee_id", "unknown employee"))?;
    if employee.status == "terminated" {
        return Err(ApiError::conflict(
            "leave cannot be requested for a terminated employee",
        ));
    }
    let type_key: String =
        sqlx::query_scalar("select key::text from leave_types where key = $1::citext")
            .bind(&body.type_key)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| ApiError::validation("type_key", "unknown leave type"))?;
    let overlaps: bool = sqlx::query_scalar(
        "select exists (select 1 from leave_requests
                         where employee_id = $1 and status in ('pending','approved')
                           and daterange(start_date, end_date, '[]') && daterange($2, $3, '[]'))",
    )
    .bind(employee_id)
    .bind(body.start_date)
    .bind(body.end_date)
    .fetch_one(&mut *tx)
    .await?;
    if overlaps {
        return Err(ApiError::conflict(
            "the employee already has leave booked in that window",
        ));
    }
    // The employee's direct manager decides; HR can still act on anyone.
    let approver_id = employee.manager_id;
    let insert: Result<Uuid, sqlx::Error> = sqlx::query_scalar(
        "insert into leave_requests (employee_id, type_key, start_date, end_date, days, reason,
                                     current_approver_id)
         values ($1, $2::citext, $3, $4, $5, $6, $7) returning id",
    )
    .bind(employee_id)
    .bind(&type_key)
    .bind(body.start_date)
    .bind(body.end_date)
    .bind(days)
    .bind(&body.reason)
    .bind(approver_id)
    .fetch_one(&mut *tx)
    .await;
    let id = match insert {
        Ok(id) => id,
        Err(err) if service::is_overlap(&err) => {
            return Err(ApiError::conflict(
                "the employee already has leave booked in that window",
            ))
        }
        Err(err) => return Err(err.into()),
    };
    let after = audit::snapshot(&mut tx, "leave_requests", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "leave.request",
        "leave_request",
        Some(id),
        None,
        after,
    )
    .await?;
    if let Some(approver) = approver_id {
        outbox::enqueue_email(
            &mut tx,
            &[approver],
            &format!("Leave request from {}", employee.full_name()),
            &format!(
                "{} requested {} leave from {} to {} ({} days). Open Bowline to approve or reject it.",
                employee.full_name(),
                type_key,
                body.start_date,
                body.end_date,
                days
            ),
        )
        .await?;
    }
    let out = fetch_leave_request(&mut tx, id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(out)))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct ApproveLeave {
    #[validate(length(max = 2000))]
    pub note: Option<String>,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct RejectLeave {
    #[validate(length(min = 1, max = 2000))]
    pub note: String,
}

#[utoipa::path(post, path = "/api/v1/hr/leave/requests/{id}/approve", tag = "hr", security(("bearer" = [])),
    request_body = ApproveLeave,
    responses((status = 200, body = LeaveRequestOut), (status = 409, body = Problem)))]
pub async fn approve_leave(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<ApproveLeave>,
) -> ApiResult<Json<LeaveRequestOut>> {
    actor.require_any(&["leave:approve:subtree", "leave:manage:all"])?;
    let mut tx = state.pool.begin().await?;
    let request = service::load_request(&mut tx, id).await?;
    if !service::may_decide(&actor, &request.employee_path) {
        return Err(ApiError::not_found("leave request"));
    }
    if request.status != "pending" {
        return Err(ApiError::transition(&request.status, "approved"));
    }
    let year = service::leave_year(request.start_date);
    let (allocated, used) =
        service::balance_for(&mut tx, request.employee_id, year, &request.type_key)
            .await?
            .ok_or_else(|| ApiError::conflict("the leave type no longer exists"))?;
    if allocated > Decimal::ZERO && used + request.days > allocated {
        return Err(ApiError::conflict(format!(
            "{} has only {} {} days left",
            request.employee_name,
            allocated - used,
            request.type_key
        )));
    }
    let before = audit::snapshot(&mut tx, "leave_requests", id).await?;
    sqlx::query(
        "update leave_requests
            set status = 'approved', decided_by = $2, decided_at = now(), decision_note = $3
          where id = $1",
    )
    .bind(id)
    .bind(actor.me())
    .bind(&body.note)
    .execute(&mut *tx)
    .await?;
    service::apply_balance(
        &mut tx,
        request.employee_id,
        year,
        &request.type_key,
        request.days,
    )
    .await?;
    let after = audit::snapshot(&mut tx, "leave_requests", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "leave.approve",
        "leave_request",
        Some(id),
        before,
        after,
    )
    .await?;
    outbox::enqueue_email(
        &mut tx,
        &[request.employee_id],
        "Your leave request was approved",
        &format!(
            "{} approved your {} leave from {} to {}.",
            actor.principal.full_name(),
            request.type_key,
            request.start_date,
            request.end_date
        ),
    )
    .await?;
    let out = fetch_leave_request(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(out))
}

#[utoipa::path(post, path = "/api/v1/hr/leave/requests/{id}/reject", tag = "hr", security(("bearer" = [])),
    request_body = RejectLeave,
    responses((status = 200, body = LeaveRequestOut), (status = 409, body = Problem)))]
pub async fn reject_leave(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<RejectLeave>,
) -> ApiResult<Json<LeaveRequestOut>> {
    actor.require_any(&["leave:approve:subtree", "leave:manage:all"])?;
    let mut tx = state.pool.begin().await?;
    let request = service::load_request(&mut tx, id).await?;
    if !service::may_decide(&actor, &request.employee_path) {
        return Err(ApiError::not_found("leave request"));
    }
    if request.status != "pending" {
        return Err(ApiError::transition(&request.status, "rejected"));
    }
    let before = audit::snapshot(&mut tx, "leave_requests", id).await?;
    sqlx::query(
        "update leave_requests
            set status = 'rejected', decided_by = $2, decided_at = now(), decision_note = $3
          where id = $1",
    )
    .bind(id)
    .bind(actor.me())
    .bind(&body.note)
    .execute(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "leave_requests", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "leave.reject",
        "leave_request",
        Some(id),
        before,
        after,
    )
    .await?;
    outbox::enqueue_email(
        &mut tx,
        &[request.employee_id],
        "Your leave request was rejected",
        &format!(
            "{} rejected your {} leave from {} to {}: {}",
            actor.principal.full_name(),
            request.type_key,
            request.start_date,
            request.end_date,
            body.note
        ),
    )
    .await?;
    let out = fetch_leave_request(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(out))
}

#[utoipa::path(post, path = "/api/v1/hr/leave/requests/{id}/cancel", tag = "hr", security(("bearer" = [])),
    responses((status = 200, body = LeaveRequestOut), (status = 409, body = Problem)))]
pub async fn cancel_leave(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<LeaveRequestOut>> {
    let mut tx = state.pool.begin().await?;
    let request = service::load_request(&mut tx, id).await?;
    let owner = request.employee_id == actor.me();
    if !owner && !actor.has("leave:manage:all") {
        return Err(ApiError::not_found("leave request"));
    }
    if request.status != "pending" && request.status != "approved" {
        return Err(ApiError::transition(&request.status, "cancelled"));
    }
    let before = audit::snapshot(&mut tx, "leave_requests", id).await?;
    sqlx::query(
        "update leave_requests
            set status = 'cancelled', decided_by = $2, decided_at = now()
          where id = $1",
    )
    .bind(id)
    .bind(actor.me())
    .execute(&mut *tx)
    .await?;
    // Days already deducted go back to the balance they came from.
    if request.status == "approved" {
        let year = service::leave_year(request.start_date);
        service::apply_balance(
            &mut tx,
            request.employee_id,
            year,
            &request.type_key,
            -request.days,
        )
        .await?;
    }
    let after = audit::snapshot(&mut tx, "leave_requests", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "leave.cancel",
        "leave_request",
        Some(id),
        before,
        after,
    )
    .await?;
    if !owner {
        outbox::enqueue_email(
            &mut tx,
            &[request.employee_id],
            "Your leave request was cancelled",
            &format!(
                "{} cancelled your {} leave from {} to {}.",
                actor.principal.full_name(),
                request.type_key,
                request.start_date,
                request.end_date
            ),
        )
        .await?;
    }
    let out = fetch_leave_request(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(out))
}

// ---------------------------------------------------------------------------
// Shifts
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct ShiftOut {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub employee_name: String,
    pub site: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub role_on_shift: Option<String>,
    pub status: String,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

const SHIFT_SELECT: &str =
    "select s.id, s.employee_id, e.first_name || ' ' || e.last_name as employee_name, s.site,
            s.starts_at, s.ends_at, s.role_on_shift, s.status, s.created_by, s.created_at,
            count(*) over() as total_count
       from shifts s join employees e on e.id = s.employee_id
      where ";

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ShiftFilter {
    pub employee_id: Option<Uuid>,
    /// First day of the window (inclusive).
    pub from: Option<NaiveDate>,
    /// Last day of the window (inclusive).
    pub to: Option<NaiveDate>,
    pub site: Option<String>,
    pub status: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/hr/shifts", tag = "hr", security(("bearer" = [])),
    params(PageQuery, ShiftFilter), responses((status = 200, body = PageOut<ShiftOut>)))]
pub async fn list_shifts(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<ShiftFilter>,
) -> ApiResult<Json<PageOut<ShiftOut>>> {
    let scope = service::roster_filter(&actor);
    let mut conn = state.pool.acquire().await?;
    if let Some(employee_id) = filter.employee_id {
        org::load_in_scope(&mut conn, &scope, employee_id).await?;
    }
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(SHIFT_SELECT);
    scope.push(&mut qb, "e");
    if let Some(employee_id) = filter.employee_id {
        qb.push(" and s.employee_id = ").push_bind(employee_id);
    }
    if let Some(from) = filter.from {
        qb.push(" and s.starts_at >= ").push_bind(from);
    }
    if let Some(to) = filter.to {
        qb.push(" and s.starts_at < (").push_bind(to).push(" + 1)");
    }
    if let Some(site) = &filter.site {
        qb.push(" and s.site = ").push_bind(site.clone());
    }
    if let Some(status) = &filter.status {
        service::check_one_of(
            "status",
            status,
            &["scheduled", "completed", "missed", "cancelled"],
        )?;
        qb.push(" and s.status = ").push_bind(status.clone());
    }
    let paging = page.page();
    qb.push(" order by s.starts_at desc limit ");
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&mut *conn).await?;
    let (items, total) = split_total::<ShiftOut>(rows)?;
    Ok(Json(PageOut::new(items, paging, total)))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateShift {
    pub employee_id: Uuid,
    #[validate(length(min = 1, max = 80))]
    pub site: String,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    #[validate(length(max = 80))]
    pub role_on_shift: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/hr/shifts", tag = "hr", security(("bearer" = [])),
    request_body = CreateShift, responses((status = 201, body = ShiftOut), (status = 403, body = Problem)))]
pub async fn create_shift(
    State(state): State<AppState>,
    actor: Actor,
    ValidatedJson(body): ValidatedJson<CreateShift>,
) -> ApiResult<(StatusCode, Json<ShiftOut>)> {
    actor.require("shifts:manage:subtree")?;
    if body.ends_at <= body.starts_at {
        return Err(ApiError::validation("ends_at", "must be after starts_at"));
    }
    let mut tx = state.pool.begin().await?;
    let employee = org::load_core(&mut tx, body.employee_id)
        .await?
        .ok_or_else(|| ApiError::validation("employee_id", "unknown employee"))?;
    let allowed = actor.principal.is_in_subtree(&employee.path)
        || service::roster_filter(&actor).scope == Scope::All;
    if !allowed {
        return Err(ApiError::forbidden(
            "shifts can only be scheduled for people who report up to you",
        ));
    }
    if employee.status == "terminated" {
        return Err(ApiError::conflict(
            "a terminated employee cannot be scheduled",
        ));
    }
    let id: Uuid = sqlx::query_scalar(
        "insert into shifts (employee_id, site, starts_at, ends_at, role_on_shift, created_by)
         values ($1, $2, $3, $4, $5, $6) returning id",
    )
    .bind(body.employee_id)
    .bind(&body.site)
    .bind(body.starts_at)
    .bind(body.ends_at)
    .bind(&body.role_on_shift)
    .bind(actor.me())
    .fetch_one(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "shifts", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "shift.create",
        "shift",
        Some(id),
        None,
        after,
    )
    .await?;
    outbox::enqueue_email(
        &mut tx,
        &[body.employee_id],
        "A new shift was scheduled for you",
        &format!(
            "You are on shift at {} from {} to {}.",
            body.site, body.starts_at, body.ends_at
        ),
    )
    .await?;
    let out: ShiftOut = sqlx::query_as(&format!("{SHIFT_SELECT} s.id = $1"))
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(out)))
}

// ---------------------------------------------------------------------------
// Attendance
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct AttendanceOut {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub employee_name: String,
    pub shift_id: Option<Uuid>,
    pub clock_in: DateTime<Utc>,
    pub clock_out: Option<DateTime<Utc>>,
    pub late: bool,
    pub source: String,
    pub created_at: DateTime<Utc>,
}

const ATTENDANCE_SELECT: &str =
    "select a.id, a.employee_id, e.first_name || ' ' || e.last_name as employee_name, a.shift_id,
            a.clock_in, a.clock_out, a.late, a.source, a.created_at, count(*) over() as total_count
       from attendance a join employees e on e.id = a.employee_id
      where ";

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct AttendanceFilter {
    pub employee_id: Option<Uuid>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
    /// `1` to return only the records where the employee arrived late.
    pub late: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/hr/attendance", tag = "hr", security(("bearer" = [])),
    params(PageQuery, AttendanceFilter), responses((status = 200, body = PageOut<AttendanceOut>)))]
pub async fn list_attendance(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<AttendanceFilter>,
) -> ApiResult<Json<PageOut<AttendanceOut>>> {
    let scope = service::roster_filter(&actor);
    let mut conn = state.pool.acquire().await?;
    if let Some(employee_id) = filter.employee_id {
        org::load_in_scope(&mut conn, &scope, employee_id).await?;
    }
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(ATTENDANCE_SELECT);
    scope.push(&mut qb, "e");
    if let Some(employee_id) = filter.employee_id {
        qb.push(" and a.employee_id = ").push_bind(employee_id);
    }
    if let Some(from) = filter.from {
        qb.push(" and a.clock_in >= ").push_bind(from);
    }
    if let Some(to) = filter.to {
        qb.push(" and a.clock_in < (").push_bind(to).push(" + 1)");
    }
    if service::truthy(filter.late.as_deref()) {
        qb.push(" and a.late");
    }
    let paging = page.page();
    qb.push(" order by a.clock_in desc limit ");
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&mut *conn).await?;
    let (items, total) = split_total::<AttendanceOut>(rows)?;
    Ok(Json(PageOut::new(items, paging, total)))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct ClockIn {
    /// The scheduled shift being started; lateness is measured against it.
    pub shift_id: Option<Uuid>,
    /// `web`, `mobile`, `kiosk` or `import`; defaults to `web`.
    pub source: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/hr/attendance/clock-in", tag = "hr", security(("bearer" = [])),
    request_body = ClockIn, responses((status = 201, body = AttendanceOut), (status = 409, body = Problem)))]
pub async fn clock_in(
    State(state): State<AppState>,
    actor: Actor,
    ValidatedJson(body): ValidatedJson<ClockIn>,
) -> ApiResult<(StatusCode, Json<AttendanceOut>)> {
    actor.require("attendance:record:self")?;
    let source = body.source.clone().unwrap_or_else(|| "web".to_string());
    service::check_one_of("source", &source, &service::ATTENDANCE_SOURCES)?;
    let now = Utc::now();
    let mut tx = state.pool.begin().await?;
    let open: Option<Uuid> = sqlx::query_scalar(
        "select id from attendance where employee_id = $1 and clock_out is null
          order by clock_in desc limit 1 for update",
    )
    .bind(actor.me())
    .fetch_optional(&mut *tx)
    .await?;
    if open.is_some() {
        return Err(ApiError::conflict("you are already clocked in"));
    }
    let mut late = false;
    if let Some(shift_id) = body.shift_id {
        let shift: Option<(Uuid, DateTime<Utc>)> =
            sqlx::query_as("select employee_id, starts_at from shifts where id = $1")
                .bind(shift_id)
                .fetch_optional(&mut *tx)
                .await?;
        let (owner, starts_at) = shift.ok_or_else(|| ApiError::not_found("shift"))?;
        if owner != actor.me() {
            return Err(ApiError::not_found("shift"));
        }
        late = now > starts_at + Duration::minutes(service::LATE_AFTER_MINUTES);
    }
    let id: Uuid = sqlx::query_scalar(
        "insert into attendance (employee_id, shift_id, clock_in, late, source)
         values ($1, $2, $3, $4, $5) returning id",
    )
    .bind(actor.me())
    .bind(body.shift_id)
    .bind(now)
    .bind(late)
    .bind(&source)
    .fetch_one(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "attendance", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "attendance.clock_in",
        "attendance",
        Some(id),
        None,
        after,
    )
    .await?;
    let out: AttendanceOut = sqlx::query_as(&format!("{ATTENDANCE_SELECT} a.id = $1"))
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(out)))
}

#[utoipa::path(post, path = "/api/v1/hr/attendance/clock-out", tag = "hr", security(("bearer" = [])),
    responses((status = 200, body = AttendanceOut), (status = 409, body = Problem)))]
pub async fn clock_out(
    State(state): State<AppState>,
    actor: Actor,
) -> ApiResult<Json<AttendanceOut>> {
    actor.require("attendance:record:self")?;
    let mut tx = state.pool.begin().await?;
    let open: Option<Uuid> = sqlx::query_scalar(
        "select id from attendance where employee_id = $1 and clock_out is null
          order by clock_in desc limit 1 for update",
    )
    .bind(actor.me())
    .fetch_optional(&mut *tx)
    .await?;
    let id = open.ok_or_else(|| ApiError::conflict("you are not clocked in"))?;
    let before = audit::snapshot(&mut tx, "attendance", id).await?;
    sqlx::query("update attendance set clock_out = now() where id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let after = audit::snapshot(&mut tx, "attendance", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "attendance.clock_out",
        "attendance",
        Some(id),
        before,
        after,
    )
    .await?;
    let out: AttendanceOut = sqlx::query_as(&format!("{ATTENDANCE_SELECT} a.id = $1"))
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(out))
}

// ---------------------------------------------------------------------------
// Employee documents
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct DocumentOut {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub employee_name: String,
    pub kind: String,
    pub title: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub uploaded_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    /// True when the caller may ask for a download URL.
    #[sqlx(default)]
    pub downloadable: bool,
}

/// The object key never leaves the API: the list is visible to managers, the file
/// behind it is not.
const DOCUMENT_SELECT: &str =
    "select d.id, d.employee_id, e.first_name || ' ' || e.last_name as employee_name, d.kind,
            d.title, d.mime_type, d.size_bytes, d.uploaded_by, d.created_at,
            count(*) over() as total_count
       from employee_documents d join employees e on e.id = d.employee_id
      where ";

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct DocumentFilter {
    pub employee_id: Option<Uuid>,
    /// `contract`, `id`, `certificate`, `payslip` or `other`.
    pub kind: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/hr/documents", tag = "hr", security(("bearer" = [])),
    params(PageQuery, DocumentFilter), responses((status = 200, body = PageOut<DocumentOut>)))]
pub async fn list_documents(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<DocumentFilter>,
) -> ApiResult<Json<PageOut<DocumentOut>>> {
    let scope = service::document_list_filter(&actor);
    let mut conn = state.pool.acquire().await?;
    if let Some(employee_id) = filter.employee_id {
        org::load_in_scope(&mut conn, &scope, employee_id).await?;
    }
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(DOCUMENT_SELECT);
    scope.push(&mut qb, "e");
    if let Some(employee_id) = filter.employee_id {
        qb.push(" and d.employee_id = ").push_bind(employee_id);
    }
    if let Some(kind) = &filter.kind {
        service::check_one_of("kind", kind, &service::DOCUMENT_KINDS)?;
        qb.push(" and d.kind = ").push_bind(kind.clone());
    }
    let paging = page.page();
    qb.push(" order by d.created_at desc limit ");
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&mut *conn).await?;
    let (mut items, total) = split_total::<DocumentOut>(rows)?;
    for item in &mut items {
        item.downloadable = service::may_download(&actor, item.employee_id);
    }
    Ok(Json(PageOut::new(items, paging, total)))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct PresignDocument {
    pub employee_id: Uuid,
    /// `contract`, `id`, `certificate`, `payslip` or `other`.
    #[validate(length(min = 1, max = 20))]
    pub kind: String,
    #[validate(length(min = 1, max = 200))]
    pub title: String,
    #[validate(length(min = 3, max = 120))]
    pub mime_type: String,
    pub size_bytes: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PresignedUpload {
    pub upload_url: String,
    pub s3_key: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PresignedDownload {
    pub url: String,
}

/// Uploading or confirming a document is limited to the employee themselves and to
/// HR; a manager may browse the list but never touch the files.
fn require_document_write(actor: &Actor, employee_id: Uuid) -> ApiResult<()> {
    if service::may_download(actor, employee_id) {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "documents can only be managed by the employee or by HR",
        ))
    }
}

fn check_upload(kind: &str, mime_type: &str, size_bytes: i64) -> ApiResult<()> {
    service::check_one_of("kind", kind, &service::DOCUMENT_KINDS)?;
    if !mime_type.contains('/') {
        return Err(ApiError::validation("mime_type", "must be a MIME type"));
    }
    if size_bytes <= 0 || size_bytes > service::MAX_DOCUMENT_BYTES {
        return Err(ApiError::validation(
            "size_bytes",
            format!(
                "must be between 1 and {} bytes",
                service::MAX_DOCUMENT_BYTES
            ),
        ));
    }
    Ok(())
}

#[utoipa::path(post, path = "/api/v1/hr/documents/presign", tag = "hr", security(("bearer" = [])),
    request_body = PresignDocument,
    responses((status = 200, body = PresignedUpload), (status = 403, body = Problem)))]
pub async fn presign_document(
    State(state): State<AppState>,
    actor: Actor,
    ValidatedJson(body): ValidatedJson<PresignDocument>,
) -> ApiResult<Json<PresignedUpload>> {
    require_document_write(&actor, body.employee_id)?;
    check_upload(&body.kind, &body.mime_type, body.size_bytes)?;
    let mut conn = state.pool.acquire().await?;
    org::load_core(&mut conn, body.employee_id)
        .await?
        .ok_or_else(|| ApiError::validation("employee_id", "unknown employee"))?;
    let s3_key = service::document_key(body.employee_id, &body.kind, &body.title);
    let upload_url = state
        .s3
        .presign_put(&state.s3.bucket_documents, &s3_key, &body.mime_type)
        .await?;
    Ok(Json(PresignedUpload { upload_url, s3_key }))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct ConfirmDocument {
    pub employee_id: Uuid,
    #[validate(length(min = 1, max = 20))]
    pub kind: String,
    #[validate(length(min = 1, max = 200))]
    pub title: String,
    /// The key returned by the presign call.
    #[validate(length(min = 1, max = 400))]
    pub s3_key: String,
    #[validate(length(min = 3, max = 120))]
    pub mime_type: String,
    pub size_bytes: i64,
}

#[utoipa::path(post, path = "/api/v1/hr/documents", tag = "hr", security(("bearer" = [])),
    request_body = ConfirmDocument,
    responses((status = 201, body = DocumentOut), (status = 409, body = Problem)))]
pub async fn confirm_document(
    State(state): State<AppState>,
    actor: Actor,
    ValidatedJson(body): ValidatedJson<ConfirmDocument>,
) -> ApiResult<(StatusCode, Json<DocumentOut>)> {
    require_document_write(&actor, body.employee_id)?;
    check_upload(&body.kind, &body.mime_type, body.size_bytes)?;
    if !body
        .s3_key
        .starts_with(&service::document_prefix(body.employee_id))
    {
        return Err(ApiError::validation(
            "s3_key",
            "does not belong to that employee",
        ));
    }
    let mut tx = state.pool.begin().await?;
    let id: Uuid = sqlx::query_scalar(
        "insert into employee_documents (employee_id, kind, title, s3_key, mime_type, size_bytes, uploaded_by)
         values ($1, $2, $3, $4, $5, $6, $7) returning id",
    )
    .bind(body.employee_id)
    .bind(&body.kind)
    .bind(&body.title)
    .bind(&body.s3_key)
    .bind(&body.mime_type)
    .bind(body.size_bytes)
    .bind(actor.me())
    .fetch_one(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "employee_documents", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "document.create",
        "employee_document",
        Some(id),
        None,
        after,
    )
    .await?;
    let mut out: DocumentOut = sqlx::query_as(&format!("{DOCUMENT_SELECT} d.id = $1"))
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    out.downloadable = service::may_download(&actor, out.employee_id);
    Ok((StatusCode::CREATED, Json(out)))
}

#[derive(sqlx::FromRow)]
struct DocumentAccessRow {
    employee_id: Uuid,
    s3_key: String,
    path: String,
    department_id: Uuid,
}

#[utoipa::path(get, path = "/api/v1/hr/documents/{id}/download", tag = "hr", security(("bearer" = [])),
    responses((status = 200, body = PresignedDownload), (status = 403, body = Problem),
        (status = 404, body = Problem)))]
pub async fn download_document(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<PresignedDownload>> {
    let mut conn = state.pool.acquire().await?;
    let row: DocumentAccessRow = sqlx::query_as(
        "select d.employee_id, d.s3_key, e.path::text as path, e.department_id
           from employee_documents d join employees e on e.id = d.employee_id
          where d.id = $1",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(|| ApiError::not_found("document"))?;
    if !service::may_download(&actor, row.employee_id) {
        // A manager sees the row in their list, so say why the file is off limits
        // instead of pretending it does not exist.
        let visible = service::document_list_filter(&actor).contains(
            row.employee_id,
            &row.path,
            row.department_id,
        );
        return Err(if visible {
            ApiError::forbidden("employee documents are private to the employee and to HR")
        } else {
            ApiError::not_found("document")
        });
    }
    // Unlike an invoice PDF these are files HR uploaded, so a missing object cannot be
    // regenerated: doing that would put a document in front of the caller that nobody
    // ever signed. Say the file is gone instead of handing out a URL that answers 404.
    if !state
        .s3
        .object_exists(&state.s3.bucket_documents, &row.s3_key)
        .await?
    {
        tracing::error!(
            document_id = %id,
            employee_id = %row.employee_id,
            s3_key = %row.s3_key,
            "employee document is recorded but its object is missing from storage"
        );
        return Err(ApiError::NotFound(
            "the stored file for this document is missing from object storage; it has to be uploaded again"
                .to_string(),
        ));
    }
    let url = state
        .s3
        .presign_get(&state.s3.bucket_documents, &row.s3_key)
        .await?;
    Ok(Json(PresignedDownload { url }))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/hr/leave/types", get(leave_types))
        .route("/hr/leave/balances", get(leave_balances))
        .route(
            "/hr/leave/requests",
            get(list_leave_requests).post(create_leave_request),
        )
        .route("/hr/leave/requests/:id/approve", post(approve_leave))
        .route("/hr/leave/requests/:id/reject", post(reject_leave))
        .route("/hr/leave/requests/:id/cancel", post(cancel_leave))
        .route("/hr/shifts", get(list_shifts).post(create_shift))
        .route("/hr/attendance", get(list_attendance))
        .route("/hr/attendance/clock-in", post(clock_in))
        .route("/hr/attendance/clock-out", post(clock_out))
        .route("/hr/documents", get(list_documents).post(confirm_document))
        .route("/hr/documents/presign", post(presign_document))
        .route("/hr/documents/:id/download", get(download_document))
}

#[derive(OpenApi)]
#[openapi(paths(
    leave_types,
    leave_balances,
    list_leave_requests,
    create_leave_request,
    approve_leave,
    reject_leave,
    cancel_leave,
    list_shifts,
    create_shift,
    list_attendance,
    clock_in,
    clock_out,
    list_documents,
    presign_document,
    confirm_document,
    download_document
))]
pub struct HrApi;
