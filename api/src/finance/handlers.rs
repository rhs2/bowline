//! Finance endpoints: chart of accounts, fiscal periods and the journal. Invoices,
//! payables, payroll and reports live in the sibling modules and are merged into the
//! router and the OpenAPI document here.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, Postgres, QueryBuilder};
use utoipa::{IntoParams, OpenApi, ToSchema};
use uuid::Uuid;
use validator::Validate;

use crate::audit;
use crate::auth::Actor;
use crate::error::{ApiError, ApiResult, Problem};
use crate::finance::ledger::{self, Posting, PostingLine};
use crate::finance::{invoices, payables, payroll, reports};
use crate::http::extract::ValidatedJson;
use crate::http::pagination::{split_total, PageOut, PageQuery};
use crate::state::AppState;

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct AccountOut {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    #[serde(rename = "type")]
    pub account_type: String,
    pub parent_id: Option<Uuid>,
    pub active: bool,
}

#[utoipa::path(get, path = "/api/v1/finance/accounts", tag = "finance", security(("bearer" = [])),
    params(PageQuery),
    responses((status = 200, body = PageOut<AccountOut>), (status = 403, body = Problem)))]
pub async fn list_accounts(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
) -> ApiResult<Json<PageOut<AccountOut>>> {
    actor.require("ledger:read")?;
    let paging = page.page();
    let rows = sqlx::query(
        "select id, code, name, type as account_type, parent_id, active,
                count(*) over() as total_count
           from accounts
          where ($3::text is null or code ilike $3 or name ilike $3)
          order by code limit $1 offset $2",
    )
    .bind(paging.limit())
    .bind(paging.offset())
    .bind(page.search())
    .fetch_all(&state.pool)
    .await?;
    let (items, total) = split_total::<AccountOut>(rows)?;
    Ok(Json(PageOut::new(items, paging, total)))
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct PeriodOut {
    pub id: Uuid,
    pub year: i16,
    pub month: i16,
    pub starts_on: NaiveDate,
    pub ends_on: NaiveDate,
    pub status: String,
    pub closed_by: Option<Uuid>,
    pub closed_at: Option<DateTime<Utc>>,
    pub entry_count: i64,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PeriodFilter {
    pub year: Option<i16>,
    pub status: Option<String>,
}

const PERIOD_SELECT: &str =
    "select p.id, p.year, p.month, p.starts_on, p.ends_on, p.status, p.closed_by,
                p.closed_at,
                (select count(*) from journal_entries e where e.period_id = p.id) as entry_count,
                count(*) over() as total_count
           from fiscal_periods p where ";

#[utoipa::path(get, path = "/api/v1/finance/periods", tag = "finance", security(("bearer" = [])),
    params(PageQuery, PeriodFilter),
    responses((status = 200, body = PageOut<PeriodOut>), (status = 403, body = Problem)))]
pub async fn list_periods(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<PeriodFilter>,
) -> ApiResult<Json<PageOut<PeriodOut>>> {
    actor.require("ledger:read")?;
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(PERIOD_SELECT);
    qb.push(" true ");
    if let Some(year) = filter.year {
        qb.push(" and p.year = ").push_bind(year);
    }
    if let Some(status) = &filter.status {
        qb.push(" and p.status = ").push_bind(status.clone());
    }
    qb.push(" order by p.year desc, p.month desc limit ");
    let paging = page.page();
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&state.pool).await?;
    let (items, total) = split_total::<PeriodOut>(rows)?;
    Ok(Json(PageOut::new(items, paging, total)))
}

async fn load_period(conn: &mut PgConnection, id: Uuid) -> ApiResult<PeriodOut> {
    sqlx::query_as(&format!("{PERIOD_SELECT} p.id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(|| ApiError::not_found("fiscal period"))
}

#[utoipa::path(post, path = "/api/v1/finance/periods/{id}/close", tag = "finance", security(("bearer" = [])),
    responses((status = 200, body = PeriodOut), (status = 409, body = Problem)))]
pub async fn close_period(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<PeriodOut>> {
    actor.require("periods:close")?;
    let mut tx = state.pool.begin().await?;
    let period = load_period(&mut tx, id).await?;
    if period.status != "open" {
        return Err(ApiError::transition(&period.status, "closed"));
    }
    let before = audit::snapshot(&mut tx, "fiscal_periods", id).await?;
    sqlx::query(
        "update fiscal_periods set status = 'closed', closed_by = $2, closed_at = now() where id = $1",
    )
    .bind(id)
    .bind(actor.me())
    .execute(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "fiscal_periods", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "period.close",
        "fiscal_period",
        Some(id),
        before,
        after,
    )
    .await?;
    let out = load_period(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(out))
}

#[utoipa::path(post, path = "/api/v1/finance/periods/{id}/reopen", tag = "finance", security(("bearer" = [])),
    responses((status = 200, body = PeriodOut), (status = 409, body = Problem)))]
pub async fn reopen_period(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<PeriodOut>> {
    actor.require("system:admin")?;
    let mut tx = state.pool.begin().await?;
    let period = load_period(&mut tx, id).await?;
    if period.status != "closed" {
        return Err(ApiError::transition(&period.status, "open"));
    }
    let before = audit::snapshot(&mut tx, "fiscal_periods", id).await?;
    sqlx::query(
        "update fiscal_periods set status = 'open', closed_by = null, closed_at = null where id = $1",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "fiscal_periods", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "period.reopen",
        "fiscal_period",
        Some(id),
        before,
        after,
    )
    .await?;
    let out = load_period(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(out))
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct JournalLineOut {
    pub id: Uuid,
    pub entry_id: Uuid,
    pub account_id: Uuid,
    pub account_code: String,
    pub account_name: String,
    pub debit: Decimal,
    pub credit: Decimal,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct JournalEntryOut {
    pub id: Uuid,
    pub entry_no: i64,
    pub period_id: Uuid,
    pub entry_date: NaiveDate,
    pub memo: String,
    pub source_type: String,
    pub source_id: Option<Uuid>,
    pub posted_by: Option<Uuid>,
    pub posted_by_name: Option<String>,
    pub posted_at: DateTime<Utc>,
    pub reverses_entry_id: Option<Uuid>,
    pub reversed_by_entry_id: Option<Uuid>,
    pub lines: Vec<JournalLineOut>,
}

#[derive(sqlx::FromRow)]
struct EntryRow {
    id: Uuid,
    entry_no: i64,
    period_id: Uuid,
    entry_date: NaiveDate,
    memo: String,
    source_type: String,
    source_id: Option<Uuid>,
    posted_by: Option<Uuid>,
    posted_by_name: Option<String>,
    posted_at: DateTime<Utc>,
    reverses_entry_id: Option<Uuid>,
    reversed_by_entry_id: Option<Uuid>,
}

const ENTRY_SELECT: &str =
    "select e.id, e.entry_no, e.period_id, e.entry_date, e.memo, e.source_type, e.source_id,
                e.posted_by, p.first_name || ' ' || p.last_name as posted_by_name, e.posted_at,
                e.reverses_entry_id, e.reversed_by_entry_id, count(*) over() as total_count
           from journal_entries e left join employees p on p.id = e.posted_by
          where ";

/// Attaches the lines to a page of entries with one extra query.
async fn with_lines(
    conn: &mut PgConnection,
    rows: Vec<EntryRow>,
) -> ApiResult<Vec<JournalEntryOut>> {
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let lines: Vec<JournalLineOut> = if ids.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as(
            "select l.id, l.entry_id, l.account_id, a.code as account_code, a.name as account_name,
                    l.debit, l.credit, l.description
               from journal_lines l join accounts a on a.id = l.account_id
              where l.entry_id = any($1) order by l.debit desc, a.code",
        )
        .bind(&ids)
        .fetch_all(conn)
        .await?
    };
    Ok(rows
        .into_iter()
        .map(|r| JournalEntryOut {
            lines: lines
                .iter()
                .filter(|l| l.entry_id == r.id)
                .map(clone_line)
                .collect(),
            id: r.id,
            entry_no: r.entry_no,
            period_id: r.period_id,
            entry_date: r.entry_date,
            memo: r.memo,
            source_type: r.source_type,
            source_id: r.source_id,
            posted_by: r.posted_by,
            posted_by_name: r.posted_by_name,
            posted_at: r.posted_at,
            reverses_entry_id: r.reverses_entry_id,
            reversed_by_entry_id: r.reversed_by_entry_id,
        })
        .collect())
}

fn clone_line(line: &JournalLineOut) -> JournalLineOut {
    JournalLineOut {
        id: line.id,
        entry_id: line.entry_id,
        account_id: line.account_id,
        account_code: line.account_code.clone(),
        account_name: line.account_name.clone(),
        debit: line.debit,
        credit: line.credit,
        description: line.description.clone(),
    }
}

async fn load_entry(conn: &mut PgConnection, id: Uuid) -> ApiResult<JournalEntryOut> {
    let rows: Vec<EntryRow> = sqlx::query_as(&format!("{ENTRY_SELECT} e.id = $1"))
        .bind(id)
        .fetch_all(&mut *conn)
        .await?;
    if rows.is_empty() {
        return Err(ApiError::not_found("journal entry"));
    }
    let mut entries = with_lines(conn, rows).await?;
    Ok(entries.remove(0))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct JournalFilter {
    pub period_id: Option<Uuid>,
    /// Account code, for example `1100`.
    pub account: Option<String>,
    pub source_type: Option<String>,
    pub source_id: Option<Uuid>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

#[utoipa::path(get, path = "/api/v1/finance/journal", tag = "finance", security(("bearer" = [])),
    params(PageQuery, JournalFilter),
    responses((status = 200, body = PageOut<JournalEntryOut>), (status = 403, body = Problem)))]
pub async fn list_journal(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<JournalFilter>,
) -> ApiResult<Json<PageOut<JournalEntryOut>>> {
    actor.require("ledger:read")?;
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(ENTRY_SELECT);
    qb.push(" true ");
    if let Some(period) = filter.period_id {
        qb.push(" and e.period_id = ").push_bind(period);
    }
    if let Some(account) = &filter.account {
        qb.push(
            " and exists (select 1 from journal_lines l join accounts a on a.id = l.account_id
                           where l.entry_id = e.id and a.code = ",
        )
        .push_bind(account.clone())
        .push(")");
    }
    if let Some(source) = &filter.source_type {
        qb.push(" and e.source_type = ").push_bind(source.clone());
    }
    if let Some(source_id) = filter.source_id {
        qb.push(" and e.source_id = ").push_bind(source_id);
    }
    if let Some(from) = filter.from {
        qb.push(" and e.entry_date >= ").push_bind(from);
    }
    if let Some(to) = filter.to {
        qb.push(" and e.entry_date <= ").push_bind(to);
    }
    if let Some(q) = page.search() {
        qb.push(" and e.memo ilike ").push_bind(q);
    }
    let order = page.order_by(&[
        ("entry_date", "e.entry_date"),
        ("entry_no", "e.entry_no"),
        ("posted_at", "e.posted_at"),
    ]);
    qb.push(format!(" order by {order}, e.entry_no desc limit "));
    let paging = page.page();
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&state.pool).await?;
    let (rows, total) = split_total::<EntryRow>(rows)?;
    let mut conn = state.pool.acquire().await?;
    let items = with_lines(&mut conn, rows).await?;
    Ok(Json(PageOut::new(items, paging, total)))
}

#[utoipa::path(get, path = "/api/v1/finance/journal/{id}", tag = "finance", security(("bearer" = [])),
    responses((status = 200, body = JournalEntryOut), (status = 404, body = Problem)))]
pub async fn get_journal_entry(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<JournalEntryOut>> {
    actor.require("ledger:read")?;
    let mut conn = state.pool.acquire().await?;
    Ok(Json(load_entry(&mut conn, id).await?))
}

/// Ceiling on the number of lines in one manual entry, so a stray request cannot ask
/// the ledger to write an unbounded statement.
const MAX_LINES: usize = 200;

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct NewJournalLine {
    #[validate(length(min = 1, max = 20))]
    pub account_code: String,
    #[serde(default)]
    pub debit: Decimal,
    #[serde(default)]
    pub credit: Decimal,
    #[validate(length(max = 200))]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct NewJournalEntry {
    pub entry_date: NaiveDate,
    #[validate(length(min = 1, max = 400))]
    pub memo: String,
    #[validate(nested)]
    pub lines: Vec<NewJournalLine>,
}

#[utoipa::path(post, path = "/api/v1/finance/journal", tag = "finance", security(("bearer" = [])),
    request_body = NewJournalEntry,
    responses((status = 201, body = JournalEntryOut), (status = 409, body = Problem),
              (status = 422, body = Problem)))]
pub async fn post_journal_entry(
    State(state): State<AppState>,
    actor: Actor,
    ValidatedJson(body): ValidatedJson<NewJournalEntry>,
) -> ApiResult<(StatusCode, Json<JournalEntryOut>)> {
    actor.require("ledger:post")?;
    if body.lines.len() > MAX_LINES {
        return Err(ApiError::validation(
            "lines",
            format!("an entry may carry at most {MAX_LINES} lines"),
        ));
    }
    let posting = Posting::new(body.entry_date, body.memo, "manual", None).with_lines(
        body.lines
            .into_iter()
            .map(|l| PostingLine {
                account_code: l.account_code,
                debit: l.debit.round_dp(2),
                credit: l.credit.round_dp(2),
                description: l.description,
            })
            .collect(),
    );
    let mut tx = state.pool.begin().await?;
    let entry = ledger::post(&mut tx, &actor.audit(), actor.me(), posting).await?;
    let out = load_entry(&mut tx, entry.id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(out)))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct ReverseEntry {
    /// Defaults to today, so an entry in a closed month reverses into the open one.
    pub entry_date: Option<NaiveDate>,
    #[validate(length(min = 1, max = 400))]
    pub memo: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/finance/journal/{id}/reverse", tag = "finance", security(("bearer" = [])),
    request_body = ReverseEntry,
    responses((status = 201, body = JournalEntryOut), (status = 409, body = Problem)))]
pub async fn reverse_journal_entry(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    body: Option<ValidatedJson<ReverseEntry>>,
) -> ApiResult<(StatusCode, Json<JournalEntryOut>)> {
    actor.require("ledger:post")?;
    let body = body.map(|ValidatedJson(b)| b);
    let entry_date = body
        .as_ref()
        .and_then(|b| b.entry_date)
        .unwrap_or_else(|| Utc::now().date_naive());
    let memo = body.and_then(|b| b.memo);
    let mut tx = state.pool.begin().await?;
    let reversal =
        ledger::reverse(&mut tx, &actor.audit(), actor.me(), id, entry_date, memo).await?;
    let out = load_entry(&mut tx, reversal.id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(out)))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/finance/accounts", get(list_accounts))
        .route("/finance/periods", get(list_periods))
        .route("/finance/periods/:id/close", post(close_period))
        .route("/finance/periods/:id/reopen", post(reopen_period))
        .route(
            "/finance/journal",
            get(list_journal).post(post_journal_entry),
        )
        .route("/finance/journal/:id", get(get_journal_entry))
        .route("/finance/journal/:id/reverse", post(reverse_journal_entry))
        .merge(invoices::routes())
        .merge(payables::routes())
        .merge(payroll::routes())
        .merge(reports::routes())
}

#[derive(OpenApi)]
#[openapi(paths(
    list_accounts,
    list_periods,
    close_period,
    reopen_period,
    list_journal,
    get_journal_entry,
    post_journal_entry,
    reverse_journal_entry,
    invoices::list_invoices,
    invoices::create_invoice,
    invoices::get_invoice,
    invoices::update_invoice,
    invoices::submit_invoice,
    invoices::approve_invoice,
    invoices::issue_invoice,
    invoices::void_invoice,
    invoices::invoice_pdf,
    invoices::record_payment,
    invoices::list_payments,
    payables::list_vendors,
    payables::create_vendor,
    payables::list_bills,
    payables::create_bill,
    payables::approve_bill,
    payables::pay_bill,
    payables::list_expenses,
    payables::submit_expense,
    payables::approve_expense,
    payables::reject_expense,
    payables::pay_expense,
    payroll::list_runs,
    payroll::create_run,
    payroll::get_run,
    payroll::approve_run,
    payroll::post_run,
    reports::trial_balance,
    reports::ar_aging,
    reports::profit_and_loss
))]
pub struct FinanceApi;
