//! Payroll runs: one run per fiscal period, one item per active employee.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgConnection;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;
use validator::Validate;

use crate::audit;
use crate::auth::Actor;
use crate::error::{ApiError, ApiResult, Problem};
use crate::finance::ledger::{self, Posting, PostingLine};
use crate::http::extract::ValidatedJson;
use crate::http::pagination::{split_total, PageOut, PageQuery};
use crate::state::AppState;

/// Flat withholding applied to gross pay. A real payroll engine would take brackets
/// and benefits from a tax table; the platform models the shape, not the tax code.
fn deduction_rate() -> Decimal {
    Decimal::new(20, 2)
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct PayrollRunOut {
    pub id: Uuid,
    pub period_id: Uuid,
    pub year: i16,
    pub month: i16,
    pub period_status: String,
    pub status: String,
    pub total_gross: Decimal,
    pub total_deductions: Decimal,
    pub total_net: Decimal,
    pub item_count: i64,
    pub created_by: Option<Uuid>,
    pub approved_by: Option<Uuid>,
    pub approved_at: Option<DateTime<Utc>>,
    pub posted_at: Option<DateTime<Utc>>,
    pub journal_entry_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct PayrollItemOut {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub employee_no: String,
    pub employee_name: String,
    pub department_name: String,
    pub gross: Decimal,
    pub deductions: Decimal,
    pub net: Decimal,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PayrollRunDetail {
    #[serde(flatten)]
    pub run: PayrollRunOut,
    pub items: Vec<PayrollItemOut>,
}

const RUN_SELECT: &str =
    "select r.id, r.period_id, p.year, p.month, p.status as period_status, r.status,
                r.total_gross, r.total_deductions, r.total_net,
                (select count(*) from payroll_items i where i.run_id = r.id) as item_count,
                r.created_by, r.approved_by, r.approved_at, r.posted_at, r.journal_entry_id,
                r.created_at, count(*) over() as total_count
           from payroll_runs r join fiscal_periods p on p.id = r.period_id
          where ";

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct RunFilter {
    pub status: Option<String>,
    pub year: Option<i16>,
}

#[utoipa::path(get, path = "/api/v1/finance/payroll/runs", tag = "finance", security(("bearer" = [])),
    params(PageQuery, RunFilter),
    responses((status = 200, body = PageOut<PayrollRunOut>), (status = 403, body = Problem)))]
pub async fn list_runs(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<RunFilter>,
) -> ApiResult<Json<PageOut<PayrollRunOut>>> {
    actor.require_any(&["payroll:read:all", "payroll:prepare", "payroll:approve"])?;
    let mut qb: sqlx::QueryBuilder<sqlx::Postgres> = sqlx::QueryBuilder::new(RUN_SELECT);
    qb.push(" true ");
    if let Some(status) = &filter.status {
        qb.push(" and r.status = ").push_bind(status.clone());
    }
    if let Some(year) = filter.year {
        qb.push(" and p.year = ").push_bind(year);
    }
    qb.push(" order by p.year desc, p.month desc limit ");
    let paging = page.page();
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&state.pool).await?;
    let (items, total) = split_total::<PayrollRunOut>(rows)?;
    Ok(Json(PageOut::new(items, paging, total)))
}

async fn load_run(conn: &mut PgConnection, id: Uuid) -> ApiResult<PayrollRunDetail> {
    let run: Option<PayrollRunOut> = sqlx::query_as(&format!("{RUN_SELECT} r.id = $1"))
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?;
    let run = run.ok_or_else(|| ApiError::not_found("payroll run"))?;
    let items: Vec<PayrollItemOut> = sqlx::query_as(
        "select i.id, i.employee_id, e.employee_no, e.first_name || ' ' || e.last_name as employee_name,
                d.name as department_name, i.gross, i.deductions, i.net
           from payroll_items i
           join employees e on e.id = i.employee_id
           join departments d on d.id = e.department_id
          where i.run_id = $1
          order by e.last_name, e.first_name",
    )
    .bind(id)
    .fetch_all(conn)
    .await?;
    Ok(PayrollRunDetail { run, items })
}

#[utoipa::path(get, path = "/api/v1/finance/payroll/runs/{id}", tag = "finance", security(("bearer" = [])),
    responses((status = 200, body = PayrollRunDetail), (status = 404, body = Problem)))]
pub async fn get_run(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<PayrollRunDetail>> {
    actor.require_any(&["payroll:read:all", "payroll:prepare", "payroll:approve"])?;
    let mut conn = state.pool.acquire().await?;
    Ok(Json(load_run(&mut conn, id).await?))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct NewPayrollRun {
    pub period_id: Uuid,
}

#[utoipa::path(post, path = "/api/v1/finance/payroll/runs", tag = "finance", security(("bearer" = [])),
    request_body = NewPayrollRun,
    responses((status = 201, body = PayrollRunDetail), (status = 409, body = Problem)))]
pub async fn create_run(
    State(state): State<AppState>,
    actor: Actor,
    ValidatedJson(body): ValidatedJson<NewPayrollRun>,
) -> ApiResult<(StatusCode, Json<PayrollRunDetail>)> {
    actor.require("payroll:prepare")?;
    let mut tx = state.pool.begin().await?;
    let period: Option<String> =
        sqlx::query_scalar("select status from fiscal_periods where id = $1")
            .bind(body.period_id)
            .fetch_optional(&mut *tx)
            .await?;
    let status = period.ok_or_else(|| ApiError::validation("period_id", "unknown period"))?;
    if status != "open" {
        return Err(ApiError::conflict("that fiscal period is closed"));
    }
    let id: Uuid = sqlx::query_scalar(
        "insert into payroll_runs (period_id, created_by) values ($1, $2) returning id",
    )
    .bind(body.period_id)
    .bind(actor.me())
    .fetch_one(&mut *tx)
    .await?;
    // Monthly pay is a twelfth of the annual base salary, less a flat withholding.
    let inserted = sqlx::query(
        "insert into payroll_items (run_id, employee_id, gross, deductions, net)
         select $1, e.id, g.gross, round(g.gross * $2, 2), g.gross - round(g.gross * $2, 2)
           from employees e
           cross join lateral (select round(e.base_salary / 12, 2) as gross) g
          where e.status = 'active'",
    )
    .bind(id)
    .bind(deduction_rate())
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if inserted == 0 {
        return Err(ApiError::conflict("there are no active employees to pay"));
    }
    sqlx::query(
        "update payroll_runs r
            set total_gross = t.gross, total_deductions = t.deductions, total_net = t.net
           from (select coalesce(sum(gross), 0) as gross, coalesce(sum(deductions), 0) as deductions,
                        coalesce(sum(net), 0) as net
                   from payroll_items where run_id = $1) t
          where r.id = $1",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "payroll_runs", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "payroll.create",
        "payroll_run",
        Some(id),
        None,
        after.map(|a| serde_json::json!({"run": a, "items": inserted})),
    )
    .await?;
    let detail = load_run(&mut tx, id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(detail)))
}

async fn lock_run(conn: &mut PgConnection, id: Uuid) -> ApiResult<String> {
    sqlx::query_scalar("select status from payroll_runs where id = $1 for update")
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(|| ApiError::not_found("payroll run"))
}

#[utoipa::path(post, path = "/api/v1/finance/payroll/runs/{id}/approve", tag = "finance", security(("bearer" = [])),
    responses((status = 200, body = PayrollRunDetail), (status = 409, body = Problem)))]
pub async fn approve_run(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<PayrollRunDetail>> {
    actor.require("payroll:approve")?;
    let mut tx = state.pool.begin().await?;
    let status = lock_run(&mut tx, id).await?;
    if status != "draft" {
        return Err(ApiError::transition(&status, "approved"));
    }
    let before = audit::snapshot(&mut tx, "payroll_runs", id).await?;
    sqlx::query(
        "update payroll_runs set status = 'approved', approved_by = $2, approved_at = now()
          where id = $1",
    )
    .bind(id)
    .bind(actor.me())
    .execute(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "payroll_runs", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "payroll.approve",
        "payroll_run",
        Some(id),
        before,
        after,
    )
    .await?;
    let detail = load_run(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(detail))
}

#[utoipa::path(post, path = "/api/v1/finance/payroll/runs/{id}/post", tag = "finance", security(("bearer" = [])),
    responses((status = 200, body = PayrollRunDetail), (status = 409, body = Problem)))]
pub async fn post_run(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<PayrollRunDetail>> {
    actor.require("payroll:approve")?;
    let mut tx = state.pool.begin().await?;
    let status = lock_run(&mut tx, id).await?;
    if status != "approved" {
        return Err(ApiError::transition(&status, "posted"));
    }
    let detail = load_run(&mut tx, id).await?;
    let ends_on: NaiveDate = sqlx::query_scalar("select ends_on from fiscal_periods where id = $1")
        .bind(detail.run.period_id)
        .fetch_one(&mut *tx)
        .await?;
    let memo = format!(
        "Payroll {}-{:02} for {} employees",
        detail.run.year, detail.run.month, detail.run.item_count
    );
    let mut lines = vec![
        PostingLine::debit(ledger::SALARIES, detail.run.total_gross, "Gross pay"),
        PostingLine::credit(ledger::SALARIES_PAYABLE, detail.run.total_net, "Net pay"),
    ];
    if detail.run.total_deductions != Decimal::ZERO {
        // Withholdings sit in taxes payable until they are remitted.
        lines.push(PostingLine::credit(
            ledger::TAXES_PAYABLE,
            detail.run.total_deductions,
            "Payroll withholdings",
        ));
    }
    let posting = Posting::new(ends_on, memo, "payroll", Some(id)).with_lines(lines);
    let entry = ledger::post(&mut tx, &actor.audit(), actor.me(), posting).await?;
    let before = audit::snapshot(&mut tx, "payroll_runs", id).await?;
    sqlx::query(
        "update payroll_runs set status = 'posted', posted_at = now(), journal_entry_id = $2
          where id = $1",
    )
    .bind(id)
    .bind(entry.id)
    .execute(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "payroll_runs", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "payroll.post",
        "payroll_run",
        Some(id),
        before,
        after,
    )
    .await?;
    let detail = load_run(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(detail))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/finance/payroll/runs", get(list_runs).post(create_run))
        .route("/finance/payroll/runs/:id", get(get_run))
        .route("/finance/payroll/runs/:id/approve", post(approve_run))
        .route("/finance/payroll/runs/:id/post", post(post_run))
}
