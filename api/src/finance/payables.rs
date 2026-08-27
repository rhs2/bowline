//! Payables: vendors, vendor bills and employee expense claims.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, Postgres, QueryBuilder};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;
use validator::Validate;

use crate::audit;
use crate::auth::Actor;
use crate::error::{ApiError, ApiResult, Problem};
use crate::finance::ledger::{self, Posting, PostingLine};
use crate::http::extract::ValidatedJson;
use crate::http::pagination::{split_total, PageOut, PageQuery};
use crate::scope::Scope;
use crate::state::AppState;

const EXPENSE_CATEGORIES: &[&str] = &["travel", "fuel", "meals", "supplies", "equipment", "other"];

// ---------------------------------------------------------------------------
// Vendors
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct VendorOut {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    #[schema(value_type = Object)]
    pub contact: serde_json::Value,
    pub active: bool,
    pub open_bills: i64,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct VendorFilter {
    pub active: Option<bool>,
}

#[utoipa::path(get, path = "/api/v1/finance/vendors", tag = "finance", security(("bearer" = [])),
    params(PageQuery, VendorFilter),
    responses((status = 200, body = PageOut<VendorOut>), (status = 403, body = Problem)))]
pub async fn list_vendors(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<VendorFilter>,
) -> ApiResult<Json<PageOut<VendorOut>>> {
    actor.require_any(&["vendors:manage", "ledger:read"])?;
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        "select v.id, v.code::text as code, v.name, v.contact, v.active,
                (select count(*) from vendor_bills b where b.vendor_id = v.id
                  and b.status in ('received','approved')) as open_bills,
                count(*) over() as total_count
           from vendors v where true ",
    );
    if let Some(active) = filter.active {
        qb.push(" and v.active = ").push_bind(active);
    }
    if let Some(q) = page.search() {
        qb.push(" and (v.name ilike ")
            .push_bind(q.clone())
            .push(" or v.code::text ilike ")
            .push_bind(q)
            .push(")");
    }
    let order = page.order_by(&[("name", "v.name"), ("code", "v.code")]);
    let order = if page.sort.is_none() {
        "v.name asc".to_string()
    } else {
        order
    };
    qb.push(format!(" order by {order} limit "));
    let paging = page.page();
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&state.pool).await?;
    let (items, total) = split_total::<VendorOut>(rows)?;
    Ok(Json(PageOut::new(items, paging, total)))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct NewVendor {
    #[validate(length(min = 2, max = 40))]
    pub code: String,
    #[validate(length(min = 1, max = 200))]
    pub name: String,
    #[schema(value_type = Object)]
    pub contact: Option<serde_json::Value>,
}

#[utoipa::path(post, path = "/api/v1/finance/vendors", tag = "finance", security(("bearer" = [])),
    request_body = NewVendor,
    responses((status = 201, body = VendorOut), (status = 409, body = Problem)))]
pub async fn create_vendor(
    State(state): State<AppState>,
    actor: Actor,
    ValidatedJson(body): ValidatedJson<NewVendor>,
) -> ApiResult<(StatusCode, Json<VendorOut>)> {
    actor.require("vendors:manage")?;
    let mut tx = state.pool.begin().await?;
    let id: Uuid = sqlx::query_scalar(
        "insert into vendors (code, name, contact) values ($1, $2, coalesce($3, '{}'::jsonb)) returning id",
    )
    .bind(&body.code)
    .bind(&body.name)
    .bind(&body.contact)
    .fetch_one(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "vendors", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "vendor.create",
        "vendor",
        Some(id),
        None,
        after,
    )
    .await?;
    let vendor = load_vendor(&mut tx, id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(vendor)))
}

async fn load_vendor(conn: &mut PgConnection, id: Uuid) -> ApiResult<VendorOut> {
    sqlx::query_as(
        "select v.id, v.code::text as code, v.name, v.contact, v.active,
                (select count(*) from vendor_bills b where b.vendor_id = v.id
                  and b.status in ('received','approved')) as open_bills
           from vendors v where v.id = $1",
    )
    .bind(id)
    .fetch_optional(conn)
    .await?
    .ok_or_else(|| ApiError::not_found("vendor"))
}

// ---------------------------------------------------------------------------
// Vendor bills
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct BillOut {
    pub id: Uuid,
    pub vendor_id: Uuid,
    pub vendor_name: String,
    pub bill_no: String,
    pub expense_account_id: Uuid,
    pub expense_account_code: String,
    pub amount: Decimal,
    pub currency: String,
    pub received_on: NaiveDate,
    pub due_on: NaiveDate,
    pub status: String,
    pub approved_by: Option<Uuid>,
    pub paid_on: Option<NaiveDate>,
    pub journal_entry_id: Option<Uuid>,
    pub payment_entry_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

const BILL_SELECT: &str = "select b.id, b.vendor_id, v.name as vendor_name, b.bill_no, b.expense_account_id,
                a.code as expense_account_code, b.amount, b.currency::text as currency, b.received_on,
                b.due_on, b.status, b.approved_by, b.paid_on, b.journal_entry_id, b.payment_entry_id,
                b.created_at, count(*) over() as total_count
           from vendor_bills b
           join vendors v on v.id = b.vendor_id
           join accounts a on a.id = b.expense_account_id
          where ";

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct BillFilter {
    pub status: Option<String>,
    pub vendor_id: Option<Uuid>,
    /// `1` restricts the list to approved bills past their due date.
    pub overdue: Option<u8>,
}

#[utoipa::path(get, path = "/api/v1/finance/bills", tag = "finance", security(("bearer" = [])),
    params(PageQuery, BillFilter),
    responses((status = 200, body = PageOut<BillOut>), (status = 403, body = Problem)))]
pub async fn list_bills(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<BillFilter>,
) -> ApiResult<Json<PageOut<BillOut>>> {
    actor.require_any(&["vendors:manage", "ledger:read"])?;
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(BILL_SELECT);
    qb.push(" true ");
    if let Some(status) = &filter.status {
        qb.push(" and b.status = ").push_bind(status.clone());
    }
    if let Some(vendor) = filter.vendor_id {
        qb.push(" and b.vendor_id = ").push_bind(vendor);
    }
    if filter.overdue == Some(1) {
        qb.push(" and b.status in ('received','approved') and b.due_on < current_date");
    }
    if let Some(q) = page.search() {
        qb.push(" and (b.bill_no ilike ")
            .push_bind(q.clone())
            .push(" or v.name ilike ")
            .push_bind(q)
            .push(")");
    }
    let order = page.order_by(&[
        ("received_on", "b.received_on"),
        ("due_on", "b.due_on"),
        ("amount", "b.amount"),
    ]);
    qb.push(format!(" order by {order} limit "));
    let paging = page.page();
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&state.pool).await?;
    let (items, total) = split_total::<BillOut>(rows)?;
    Ok(Json(PageOut::new(items, paging, total)))
}

async fn load_bill(conn: &mut PgConnection, id: Uuid) -> ApiResult<BillOut> {
    sqlx::query_as(&format!("{BILL_SELECT} b.id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(|| ApiError::not_found("vendor bill"))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct NewBill {
    pub vendor_id: Uuid,
    #[validate(length(min = 1, max = 80))]
    pub bill_no: String,
    /// Chart of accounts code the cost lands on, for example `5000`.
    #[validate(length(min = 1, max = 20))]
    pub expense_account_code: String,
    pub amount: Decimal,
    #[validate(length(equal = 3))]
    pub currency: Option<String>,
    pub received_on: NaiveDate,
    pub due_on: NaiveDate,
}

#[utoipa::path(post, path = "/api/v1/finance/bills", tag = "finance", security(("bearer" = [])),
    request_body = NewBill,
    responses((status = 201, body = BillOut), (status = 422, body = Problem)))]
pub async fn create_bill(
    State(state): State<AppState>,
    actor: Actor,
    ValidatedJson(body): ValidatedJson<NewBill>,
) -> ApiResult<(StatusCode, Json<BillOut>)> {
    actor.require("vendors:manage")?;
    let amount = body.amount.round_dp(2);
    if amount <= Decimal::ZERO {
        return Err(ApiError::validation("amount", "must be greater than zero"));
    }
    if body.due_on < body.received_on {
        return Err(ApiError::validation(
            "due_on",
            "cannot be before received_on",
        ));
    }
    let mut tx = state.pool.begin().await?;
    let account_id: Uuid = sqlx::query_scalar("select id from accounts where code = $1 and active")
        .bind(&body.expense_account_code)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            ApiError::validation("expense_account_code", "unknown or inactive account")
        })?;
    let id: Uuid = sqlx::query_scalar(
        "insert into vendor_bills (vendor_id, bill_no, expense_account_id, amount, currency,
                                   received_on, due_on)
         values ($1, $2, $3, $4, coalesce($5, 'USD'), $6, $7) returning id",
    )
    .bind(body.vendor_id)
    .bind(&body.bill_no)
    .bind(account_id)
    .bind(amount)
    .bind(body.currency.as_ref().map(|c| c.to_uppercase()))
    .bind(body.received_on)
    .bind(body.due_on)
    .fetch_one(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "vendor_bills", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "bill.create",
        "vendor_bill",
        Some(id),
        None,
        after,
    )
    .await?;
    let bill = load_bill(&mut tx, id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(bill)))
}

#[utoipa::path(post, path = "/api/v1/finance/bills/{id}/approve", tag = "finance", security(("bearer" = [])),
    responses((status = 200, body = BillOut), (status = 409, body = Problem)))]
pub async fn approve_bill(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<BillOut>> {
    actor.require("vendors:manage")?;
    let mut tx = state.pool.begin().await?;
    let status: String =
        sqlx::query_scalar("select status from vendor_bills where id = $1 for update")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| ApiError::not_found("vendor bill"))?;
    if status != "received" {
        return Err(ApiError::transition(&status, "approved"));
    }
    let bill = load_bill(&mut tx, id).await?;
    // Costs are recognised on the date the bill was received, which is what keeps the
    // expense in the month it belongs to.
    let posting = Posting::new(
        bill.received_on,
        format!("Bill {} from {}", bill.bill_no, bill.vendor_name),
        "bill",
        Some(id),
    )
    .with_lines(vec![
        PostingLine::debit(
            &bill.expense_account_code,
            bill.amount,
            format!("Bill {}", bill.bill_no),
        ),
        PostingLine::credit(
            ledger::ACCOUNTS_PAYABLE,
            bill.amount,
            format!("Bill {}", bill.bill_no),
        ),
    ]);
    let entry = ledger::post(&mut tx, &actor.audit(), actor.me(), posting).await?;
    let before = audit::snapshot(&mut tx, "vendor_bills", id).await?;
    sqlx::query(
        "update vendor_bills set status = 'approved', approved_by = $2, journal_entry_id = $3
          where id = $1",
    )
    .bind(id)
    .bind(actor.me())
    .bind(entry.id)
    .execute(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "vendor_bills", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "bill.approve",
        "vendor_bill",
        Some(id),
        before,
        after,
    )
    .await?;
    let bill = load_bill(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(bill))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct PayBill {
    /// Defaults to today; must fall inside an open fiscal period.
    pub paid_on: Option<NaiveDate>,
}

#[utoipa::path(post, path = "/api/v1/finance/bills/{id}/pay", tag = "finance", security(("bearer" = [])),
    request_body = PayBill,
    responses((status = 200, body = BillOut), (status = 409, body = Problem)))]
pub async fn pay_bill(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    body: Option<ValidatedJson<PayBill>>,
) -> ApiResult<Json<BillOut>> {
    actor.require("expenses:approve:finance")?;
    let paid_on = body
        .and_then(|ValidatedJson(b)| b.paid_on)
        .unwrap_or_else(|| Utc::now().date_naive());
    let mut tx = state.pool.begin().await?;
    let status: String =
        sqlx::query_scalar("select status from vendor_bills where id = $1 for update")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| ApiError::not_found("vendor bill"))?;
    if status != "approved" {
        return Err(ApiError::transition(&status, "paid"));
    }
    let bill = load_bill(&mut tx, id).await?;
    let posting = Posting::new(
        paid_on,
        format!("Payment of bill {} to {}", bill.bill_no, bill.vendor_name),
        "bill",
        Some(id),
    )
    .with_lines(vec![
        PostingLine::debit(
            ledger::ACCOUNTS_PAYABLE,
            bill.amount,
            format!("Bill {}", bill.bill_no),
        ),
        PostingLine::credit(ledger::CASH, bill.amount, format!("Bill {}", bill.bill_no)),
    ]);
    let entry = ledger::post(&mut tx, &actor.audit(), actor.me(), posting).await?;
    let before = audit::snapshot(&mut tx, "vendor_bills", id).await?;
    sqlx::query(
        "update vendor_bills set status = 'paid', paid_on = $2, payment_entry_id = $3 where id = $1",
    )
    .bind(id)
    .bind(paid_on)
    .bind(entry.id)
    .execute(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "vendor_bills", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "bill.pay",
        "vendor_bill",
        Some(id),
        before,
        after,
    )
    .await?;
    let bill = load_bill(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(bill))
}

// ---------------------------------------------------------------------------
// Expense claims
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct ExpenseOut {
    pub id: Uuid,
    pub employee_id: Uuid,
    pub employee_name: String,
    pub department_id: Uuid,
    pub department_name: String,
    pub category: String,
    pub expense_account_id: Uuid,
    pub expense_account_code: String,
    pub amount: Decimal,
    pub currency: String,
    pub incurred_on: NaiveDate,
    pub description: String,
    pub receipt_s3_key: Option<String>,
    pub status: String,
    pub manager_approved_by: Option<Uuid>,
    pub finance_approved_by: Option<Uuid>,
    pub rejected_by: Option<Uuid>,
    pub rejection_note: Option<String>,
    pub journal_entry_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const EXPENSE_SELECT: &str = "select x.id, x.employee_id, e.first_name || ' ' || e.last_name as employee_name,
                x.department_id, d.name as department_name, x.category, x.expense_account_id,
                a.code as expense_account_code, x.amount, x.currency::text as currency, x.incurred_on,
                x.description, x.receipt_s3_key, x.status, x.manager_approved_by, x.finance_approved_by,
                x.rejected_by, x.rejection_note, x.journal_entry_id, x.created_at, x.updated_at,
                count(*) over() as total_count
           from expenses x
           join employees e on e.id = x.employee_id
           join departments d on d.id = x.department_id
           join accounts a on a.id = x.expense_account_id
          where ";

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ExpenseFilter {
    pub status: Option<String>,
    pub employee_id: Option<Uuid>,
    pub category: Option<String>,
    /// `1` lists only the claims waiting on this caller's approval step.
    pub pending_for_me: Option<u8>,
}

#[utoipa::path(get, path = "/api/v1/finance/expenses", tag = "finance", security(("bearer" = [])),
    params(PageQuery, ExpenseFilter),
    responses((status = 200, body = PageOut<ExpenseOut>), (status = 403, body = Problem)))]
pub async fn list_expenses(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<ExpenseFilter>,
) -> ApiResult<Json<PageOut<ExpenseOut>>> {
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(EXPENSE_SELECT);
    if filter.pending_for_me == Some(1) {
        let manager = actor.has("expenses:approve:subtree");
        let finance = actor.has("expenses:approve:finance");
        if !manager && !finance {
            return Err(ApiError::forbidden(
                "requires expenses:approve:subtree or expenses:approve:finance",
            ));
        }
        qb.push(" (false ");
        if manager {
            qb.push(" or (x.status = 'submitted' and x.employee_id <> ")
                .push_bind(actor.me())
                .push(" and e.path <@ ")
                .push_bind(actor.principal.path.clone())
                .push("::ltree) ");
        }
        if finance {
            qb.push(" or x.status = 'manager_approved' ");
        }
        qb.push(") ");
    } else {
        // Finance sees everything, managers see their subtree, everyone else sees
        // their own claims.
        let scope = if actor.has("expenses:approve:finance") || actor.has("reports:read:all") {
            Scope::All
        } else if actor.has("expenses:approve:subtree") {
            Scope::Subtree
        } else {
            Scope::Own
        };
        actor.filter(scope).push(&mut qb, "e");
    }
    if let Some(status) = &filter.status {
        qb.push(" and x.status = ").push_bind(status.clone());
    }
    if let Some(employee) = filter.employee_id {
        qb.push(" and x.employee_id = ").push_bind(employee);
    }
    if let Some(category) = &filter.category {
        qb.push(" and x.category = ").push_bind(category.clone());
    }
    if let Some(q) = page.search() {
        qb.push(" and x.description ilike ").push_bind(q);
    }
    let order = page.order_by(&[
        ("created_at", "x.created_at"),
        ("incurred_on", "x.incurred_on"),
        ("amount", "x.amount"),
        ("status", "x.status"),
    ]);
    qb.push(format!(" order by {order} limit "));
    let paging = page.page();
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&state.pool).await?;
    let (items, total) = split_total::<ExpenseOut>(rows)?;
    Ok(Json(PageOut::new(items, paging, total)))
}

async fn load_expense(conn: &mut PgConnection, id: Uuid) -> ApiResult<ExpenseOut> {
    sqlx::query_as(&format!("{EXPENSE_SELECT} x.id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(|| ApiError::not_found("expense claim"))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct NewExpense {
    /// `travel`, `fuel`, `meals`, `supplies`, `equipment` or `other`.
    pub category: String,
    pub amount: Decimal,
    #[validate(length(equal = 3))]
    pub currency: Option<String>,
    pub incurred_on: NaiveDate,
    #[validate(length(min = 1, max = 1000))]
    pub description: String,
    #[validate(length(max = 400))]
    pub receipt_s3_key: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/finance/expenses", tag = "finance", security(("bearer" = [])),
    request_body = NewExpense,
    responses((status = 201, body = ExpenseOut), (status = 422, body = Problem)))]
pub async fn submit_expense(
    State(state): State<AppState>,
    actor: Actor,
    ValidatedJson(body): ValidatedJson<NewExpense>,
) -> ApiResult<(StatusCode, Json<ExpenseOut>)> {
    actor.require("expenses:submit")?;
    if !EXPENSE_CATEGORIES.contains(&body.category.as_str()) {
        return Err(ApiError::validation(
            "category",
            format!("must be one of {}", EXPENSE_CATEGORIES.join(", ")),
        ));
    }
    let amount = body.amount.round_dp(2);
    if amount <= Decimal::ZERO {
        return Err(ApiError::validation("amount", "must be greater than zero"));
    }
    if body.incurred_on > Utc::now().date_naive() {
        return Err(ApiError::validation(
            "incurred_on",
            "cannot be in the future",
        ));
    }
    let mut tx = state.pool.begin().await?;
    let account_id =
        ledger::account_id(&mut tx, ledger::expense_account_for(&body.category)).await?;
    let id: Uuid = sqlx::query_scalar(
        "insert into expenses (employee_id, department_id, category, expense_account_id, amount,
                               currency, incurred_on, description, receipt_s3_key)
         values ($1, $2, $3, $4, $5, coalesce($6, 'USD'), $7, $8, $9) returning id",
    )
    .bind(actor.me())
    .bind(actor.principal.department_id)
    .bind(&body.category)
    .bind(account_id)
    .bind(amount)
    .bind(body.currency.as_ref().map(|c| c.to_uppercase()))
    .bind(body.incurred_on)
    .bind(&body.description)
    .bind(&body.receipt_s3_key)
    .fetch_one(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "expenses", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "expense.submit",
        "expense",
        Some(id),
        None,
        after,
    )
    .await?;
    let expense = load_expense(&mut tx, id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(expense)))
}

#[derive(sqlx::FromRow)]
struct ClaimantRow {
    status: String,
    employee_id: Uuid,
    path: String,
}

async fn lock_claim(conn: &mut PgConnection, id: Uuid) -> ApiResult<ClaimantRow> {
    let claim: Option<ClaimantRow> = sqlx::query_as(
        "select x.status, x.employee_id, e.path::text as path
           from expenses x join employees e on e.id = x.employee_id
          where x.id = $1 for update of x",
    )
    .bind(id)
    .fetch_optional(conn)
    .await?;
    claim.ok_or_else(|| ApiError::not_found("expense claim"))
}

/// The approval step is decided by where the claim is and what the caller holds, never
/// by the request body.
fn next_step(actor: &Actor, claim: &ClaimantRow) -> ApiResult<&'static str> {
    match claim.status.as_str() {
        "submitted" => {
            if !actor.has("expenses:approve:subtree") {
                return Err(ApiError::forbidden(
                    "the first approval step requires expenses:approve:subtree",
                ));
            }
            if claim.employee_id == actor.me() {
                return Err(ApiError::forbidden("you cannot approve your own claim"));
            }
            if !actor.principal.is_in_subtree(&claim.path) {
                return Err(ApiError::forbidden("the claimant does not report to you"));
            }
            Ok("manager_approved")
        }
        "manager_approved" => {
            if !actor.has("expenses:approve:finance") {
                return Err(ApiError::forbidden(
                    "the finance approval step requires expenses:approve:finance",
                ));
            }
            Ok("finance_approved")
        }
        other => Err(ApiError::transition(other, "approved")),
    }
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct ApprovalNote {
    #[validate(length(min = 1, max = 1000))]
    pub note: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/finance/expenses/{id}/approve", tag = "finance", security(("bearer" = [])),
    request_body = ApprovalNote,
    responses((status = 200, body = ExpenseOut), (status = 403, body = Problem),
              (status = 409, body = Problem)))]
pub async fn approve_expense(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    body: Option<ValidatedJson<ApprovalNote>>,
) -> ApiResult<Json<ExpenseOut>> {
    let note = body.and_then(|ValidatedJson(b)| b.note);
    let mut tx = state.pool.begin().await?;
    let claim = lock_claim(&mut tx, id).await?;
    let next = next_step(&actor, &claim)?;
    let before = audit::snapshot(&mut tx, "expenses", id).await?;
    let column = if next == "manager_approved" {
        "manager_approved_by"
    } else {
        "finance_approved_by"
    };
    sqlx::query(&format!(
        "update expenses set status = $2, {column} = $3 where id = $1"
    ))
    .bind(id)
    .bind(next)
    .bind(actor.me())
    .execute(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "expenses", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "expense.approve",
        "expense",
        Some(id),
        before,
        after.map(|a| serde_json::json!({"expense": a, "step": next, "note": note})),
    )
    .await?;
    let expense = load_expense(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(expense))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct RejectExpense {
    #[validate(length(min = 1, max = 1000))]
    pub note: String,
}

#[utoipa::path(post, path = "/api/v1/finance/expenses/{id}/reject", tag = "finance", security(("bearer" = [])),
    request_body = RejectExpense,
    responses((status = 200, body = ExpenseOut), (status = 403, body = Problem),
              (status = 409, body = Problem)))]
pub async fn reject_expense(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<RejectExpense>,
) -> ApiResult<Json<ExpenseOut>> {
    let mut tx = state.pool.begin().await?;
    let claim = lock_claim(&mut tx, id).await?;
    // Rejecting takes the same standing as approving the step the claim is waiting on.
    next_step(&actor, &claim)?;
    let before = audit::snapshot(&mut tx, "expenses", id).await?;
    sqlx::query(
        "update expenses set status = 'rejected', rejected_by = $2, rejection_note = $3 where id = $1",
    )
    .bind(id)
    .bind(actor.me())
    .bind(&body.note)
    .execute(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "expenses", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "expense.reject",
        "expense",
        Some(id),
        before,
        after,
    )
    .await?;
    let expense = load_expense(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(expense))
}

#[utoipa::path(post, path = "/api/v1/finance/expenses/{id}/pay", tag = "finance", security(("bearer" = [])),
    responses((status = 200, body = ExpenseOut), (status = 409, body = Problem)))]
pub async fn pay_expense(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ExpenseOut>> {
    actor.require("expenses:approve:finance")?;
    let mut tx = state.pool.begin().await?;
    let claim = lock_claim(&mut tx, id).await?;
    if claim.status != "finance_approved" {
        return Err(ApiError::transition(&claim.status, "paid"));
    }
    let expense = load_expense(&mut tx, id).await?;
    let posting = Posting::new(
        Utc::now().date_naive(),
        format!(
            "Expense claim {} for {}",
            expense.category, expense.employee_name
        ),
        "expense",
        Some(id),
    )
    .with_lines(vec![
        PostingLine::debit(
            &expense.expense_account_code,
            expense.amount,
            expense.description.clone(),
        ),
        PostingLine::credit(
            ledger::CASH,
            expense.amount,
            format!("Reimbursement to {}", expense.employee_name),
        ),
    ]);
    let entry = ledger::post(&mut tx, &actor.audit(), actor.me(), posting).await?;
    let before = audit::snapshot(&mut tx, "expenses", id).await?;
    sqlx::query("update expenses set status = 'paid', journal_entry_id = $2 where id = $1")
        .bind(id)
        .bind(entry.id)
        .execute(&mut *tx)
        .await?;
    let after = audit::snapshot(&mut tx, "expenses", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "expense.pay",
        "expense",
        Some(id),
        before,
        after,
    )
    .await?;
    let expense = load_expense(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(expense))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/finance/vendors", get(list_vendors).post(create_vendor))
        .route("/finance/bills", get(list_bills).post(create_bill))
        .route("/finance/bills/:id/approve", post(approve_bill))
        .route("/finance/bills/:id/pay", post(pay_bill))
        .route("/finance/expenses", get(list_expenses).post(submit_expense))
        .route("/finance/expenses/:id/approve", post(approve_expense))
        .route("/finance/expenses/:id/reject", post(reject_expense))
        .route("/finance/expenses/:id/pay", post(pay_expense))
}
