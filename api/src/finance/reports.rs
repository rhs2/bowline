//! Financial reports, read from the SQL views that ship with the schema:
//! `trial_balance`, `ar_aging` and `profit_and_loss`.

use axum::extract::{Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{Datelike, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::auth::Actor;
use crate::error::{ApiError, ApiResult, Problem};
use crate::state::AppState;

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct TrialBalanceRow {
    pub code: String,
    pub name: String,
    #[serde(rename = "type")]
    pub account_type: String,
    pub debit: Decimal,
    pub credit: Decimal,
    pub balance: Decimal,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TrialBalanceReport {
    pub rows: Vec<TrialBalanceRow>,
    pub total_debit: Decimal,
    pub total_credit: Decimal,
    /// True when the ledger as a whole is in balance, which it always should be.
    pub balanced: bool,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TrialBalanceQuery {
    /// `1` drops accounts that have never been posted to.
    pub nonzero: Option<u8>,
}

#[utoipa::path(get, path = "/api/v1/finance/reports/trial-balance", tag = "finance",
    security(("bearer" = [])), params(TrialBalanceQuery),
    responses((status = 200, body = TrialBalanceReport), (status = 403, body = Problem)))]
pub async fn trial_balance(
    State(state): State<AppState>,
    actor: Actor,
    Query(query): Query<TrialBalanceQuery>,
) -> ApiResult<Json<TrialBalanceReport>> {
    actor.require("ledger:read")?;
    let rows: Vec<TrialBalanceRow> = sqlx::query_as(
        "select code, name, type as account_type, debit, credit, balance from trial_balance
          where $1 = 0 or debit <> 0 or credit <> 0
          order by code",
    )
    .bind(i32::from(query.nonzero.unwrap_or(0)))
    .fetch_all(&state.pool)
    .await?;
    let total_debit: Decimal = rows.iter().map(|r| r.debit).sum();
    let total_credit: Decimal = rows.iter().map(|r| r.credit).sum();
    Ok(Json(TrialBalanceReport {
        rows,
        total_debit,
        total_credit,
        balanced: total_debit == total_credit,
    }))
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct ArAgingRow {
    pub invoice_id: Uuid,
    pub invoice_no: String,
    pub customer_id: Uuid,
    pub customer_name: String,
    pub due_date: Option<NaiveDate>,
    pub outstanding: Decimal,
    pub days_overdue: i32,
    pub bucket: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ArAgingBucket {
    pub bucket: String,
    pub invoices: i64,
    pub outstanding: Decimal,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ArAgingReport {
    pub as_of: NaiveDate,
    pub rows: Vec<ArAgingRow>,
    pub buckets: Vec<ArAgingBucket>,
    pub total_outstanding: Decimal,
    pub total_overdue: Decimal,
}

/// Bucket order used in the response, oldest debt last.
const BUCKETS: &[&str] = &["current", "1-30", "31-60", "61-90", "90+"];

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ArAgingQuery {
    pub customer_id: Option<Uuid>,
    /// `xlsx` streams the spreadsheet rendered by the billing service.
    pub format: Option<String>,
    /// Reporting date passed to the billing service; defaults to today.
    pub as_of: Option<NaiveDate>,
}

#[utoipa::path(get, path = "/api/v1/finance/reports/ar-aging", tag = "finance",
    security(("bearer" = [])), params(ArAgingQuery),
    responses((status = 200, body = ArAgingReport,
               description = "Aged receivables; `?format=xlsx` returns a spreadsheet instead"),
              (status = 403, body = Problem), (status = 502, body = Problem)))]
pub async fn ar_aging(
    State(state): State<AppState>,
    actor: Actor,
    Query(query): Query<ArAgingQuery>,
) -> ApiResult<Response> {
    actor.require("ledger:read")?;
    let as_of = query.as_of.unwrap_or_else(|| Utc::now().date_naive());
    if query.format.as_deref() == Some("xlsx") {
        let (bytes, content_type) = state
            .billing
            .ar_aging_xlsx(&as_of.to_string())
            .await
            .map_err(|e| {
                tracing::warn!(error = %e, "billing could not render the AR aging spreadsheet");
                ApiError::internal_msg("the billing service could not render the spreadsheet")
            })?;
        let mut response = (StatusCode::OK, bytes).into_response();
        let headers = response.headers_mut();
        if let Ok(value) = HeaderValue::from_str(&content_type) {
            headers.insert(header::CONTENT_TYPE, value);
        }
        if let Ok(value) =
            HeaderValue::from_str(&format!("attachment; filename=\"ar-aging-{as_of}.xlsx\""))
        {
            headers.insert(header::CONTENT_DISPOSITION, value);
        }
        return Ok(response);
    }
    let rows: Vec<ArAgingRow> = sqlx::query_as(
        "select invoice_id, invoice_no, customer_id, customer_name, due_date, outstanding,
                days_overdue::int as days_overdue, bucket
           from ar_aging
          where ($1::uuid is null or customer_id = $1)
          order by days_overdue desc, invoice_no",
    )
    .bind(query.customer_id)
    .fetch_all(&state.pool)
    .await?;
    let buckets = BUCKETS
        .iter()
        .map(|bucket| ArAgingBucket {
            bucket: (*bucket).to_string(),
            invoices: rows.iter().filter(|r| r.bucket == *bucket).count() as i64,
            outstanding: rows
                .iter()
                .filter(|r| r.bucket == *bucket)
                .map(|r| r.outstanding)
                .sum(),
        })
        .collect();
    let total_outstanding: Decimal = rows.iter().map(|r| r.outstanding).sum();
    let total_overdue: Decimal = rows
        .iter()
        .filter(|r| r.days_overdue > 0)
        .map(|r| r.outstanding)
        .sum();
    Ok(Json(ArAgingReport {
        as_of,
        rows,
        buckets,
        total_outstanding,
        total_overdue,
    })
    .into_response())
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct PnlRow {
    pub code: String,
    pub name: String,
    #[serde(rename = "type")]
    pub account_type: String,
    pub amount: Decimal,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PnlReport {
    pub year: i16,
    pub month: Option<i16>,
    pub revenue: Vec<PnlRow>,
    pub expenses: Vec<PnlRow>,
    pub total_revenue: Decimal,
    pub total_expenses: Decimal,
    pub net_income: Decimal,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PnlQuery {
    /// Defaults to the current calendar year.
    pub year: Option<i16>,
    /// Omit for the whole year.
    pub month: Option<i16>,
}

#[utoipa::path(get, path = "/api/v1/finance/reports/pnl", tag = "finance", security(("bearer" = [])),
    params(PnlQuery),
    responses((status = 200, body = PnlReport), (status = 403, body = Problem),
              (status = 422, body = Problem)))]
pub async fn profit_and_loss(
    State(state): State<AppState>,
    actor: Actor,
    Query(query): Query<PnlQuery>,
) -> ApiResult<Json<PnlReport>> {
    actor.require("ledger:read")?;
    let year = query.year.unwrap_or_else(|| Utc::now().year() as i16);
    if let Some(month) = query.month {
        if !(1..=12).contains(&month) {
            return Err(ApiError::validation("month", "must be between 1 and 12"));
        }
    }
    let rows: Vec<PnlRow> = sqlx::query_as(
        "select code, name, type as account_type, sum(amount) as amount
           from profit_and_loss
          where year = $1 and ($2::smallint is null or month = $2)
          group by type, code, name
          order by code",
    )
    .bind(year)
    .bind(query.month)
    .fetch_all(&state.pool)
    .await?;
    let (revenue, expenses): (Vec<PnlRow>, Vec<PnlRow>) =
        rows.into_iter().partition(|r| r.account_type == "revenue");
    let total_revenue: Decimal = revenue.iter().map(|r| r.amount).sum();
    let total_expenses: Decimal = expenses.iter().map(|r| r.amount).sum();
    Ok(Json(PnlReport {
        year,
        month: query.month,
        revenue,
        expenses,
        total_revenue,
        total_expenses,
        net_income: total_revenue - total_expenses,
    }))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/finance/reports/trial-balance", get(trial_balance))
        .route("/finance/reports/ar-aging", get(ar_aging))
        .route("/finance/reports/pnl", get(profit_and_loss))
}
