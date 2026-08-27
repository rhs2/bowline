//! Receivables: invoice drafting, approval, issue, void and customer payments.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
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
use crate::org::service as org;
use crate::state::AppState;

/// Statuses that still allow a draft to be edited.
const EDITABLE: &[&str] = &["draft"];
const DEFAULT_DUE_DAYS: i32 = 30;
/// Ceiling on the number of lines in one invoice.
const MAX_LINES: usize = 200;

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct InvoiceSummary {
    pub id: Uuid,
    pub invoice_no: String,
    pub customer_id: Uuid,
    pub customer_name: String,
    pub shipment_id: Option<Uuid>,
    pub shipment_reference: Option<String>,
    pub status: String,
    pub issue_date: Option<NaiveDate>,
    pub due_date: Option<NaiveDate>,
    pub currency: String,
    pub subtotal: Decimal,
    pub tax: Decimal,
    pub total: Decimal,
    pub amount_paid: Decimal,
    pub outstanding: Decimal,
    pub overdue: bool,
    pub has_pdf: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct InvoiceLineOut {
    pub id: Uuid,
    pub invoice_id: Uuid,
    pub seq: i16,
    pub description: String,
    pub quantity: Decimal,
    pub unit_price: Decimal,
    pub tax_rate: Decimal,
    pub amount: Decimal,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct PaymentOut {
    pub id: Uuid,
    pub invoice_id: Uuid,
    pub received_on: NaiveDate,
    pub amount: Decimal,
    pub method: String,
    pub reference: Option<String>,
    pub recorded_by: Option<Uuid>,
    pub journal_entry_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

/// The customer as the invoice page needs it: enough to print the document and to
/// show who is being billed without a second round trip.
#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct CustomerBlock {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub contact_name: Option<String>,
    pub contact_email: Option<String>,
    pub phone: Option<String>,
    #[schema(value_type = Object)]
    pub billing_address: serde_json::Value,
    pub currency: String,
    pub status: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct InvoiceDetail {
    #[serde(flatten)]
    pub summary: InvoiceSummary,
    pub notes: Option<String>,
    pub created_by: Option<Uuid>,
    pub approved_by: Option<Uuid>,
    pub issued_by: Option<Uuid>,
    pub journal_entry_id: Option<Uuid>,
    /// Present once the billing service has rendered the document.
    pub pdf_s3_key: Option<String>,
    pub customer: CustomerBlock,
    pub lines: Vec<InvoiceLineOut>,
    pub payments: Vec<PaymentOut>,
}

const INVOICE_SELECT: &str =
    "select i.id, i.invoice_no, i.customer_id, c.name as customer_name, i.shipment_id,
                s.reference as shipment_reference, i.status, i.issue_date, i.due_date,
                i.currency::text as currency, i.subtotal, i.tax, i.total, i.amount_paid,
                i.total - i.amount_paid as outstanding,
                (i.status in ('issued','partially_paid') and i.due_date < current_date) as overdue,
                i.pdf_s3_key is not null as has_pdf, i.created_at, i.updated_at,
                count(*) over() as total_count
           from invoices i
           join customers c on c.id = i.customer_id
           left join shipments s on s.id = i.shipment_id
          where ";

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct InvoiceFilter {
    pub status: Option<String>,
    pub customer_id: Option<Uuid>,
    pub shipment_id: Option<Uuid>,
    /// `1` restricts the list to issued invoices past their due date.
    pub overdue: Option<u8>,
}

#[utoipa::path(get, path = "/api/v1/finance/invoices", tag = "finance", security(("bearer" = [])),
    params(PageQuery, InvoiceFilter),
    responses((status = 200, body = PageOut<InvoiceSummary>), (status = 403, body = Problem)))]
pub async fn list_invoices(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<InvoiceFilter>,
) -> ApiResult<Json<PageOut<InvoiceSummary>>> {
    actor.require_any(&["ledger:read", "customers:read"])?;
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(INVOICE_SELECT);
    qb.push(" true ");
    if let Some(status) = &filter.status {
        qb.push(" and i.status = ").push_bind(status.clone());
    }
    if let Some(customer) = filter.customer_id {
        qb.push(" and i.customer_id = ").push_bind(customer);
    }
    if let Some(shipment) = filter.shipment_id {
        qb.push(" and i.shipment_id = ").push_bind(shipment);
    }
    if filter.overdue == Some(1) {
        qb.push(" and i.status in ('issued','partially_paid') and i.due_date < current_date");
    }
    if let Some(q) = page.search() {
        qb.push(" and (i.invoice_no ilike ")
            .push_bind(q.clone())
            .push(" or c.name ilike ")
            .push_bind(q)
            .push(")");
    }
    let order = page.order_by(&[
        ("created_at", "i.created_at"),
        ("invoice_no", "i.invoice_no"),
        ("due_date", "i.due_date"),
        ("total", "i.total"),
        ("status", "i.status"),
    ]);
    qb.push(format!(" order by {order} limit "));
    let paging = page.page();
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&state.pool).await?;
    let (items, total) = split_total::<InvoiceSummary>(rows)?;
    Ok(Json(PageOut::new(items, paging, total)))
}

#[derive(sqlx::FromRow)]
struct InvoiceExtra {
    notes: Option<String>,
    created_by: Option<Uuid>,
    approved_by: Option<Uuid>,
    issued_by: Option<Uuid>,
    journal_entry_id: Option<Uuid>,
    pdf_s3_key: Option<String>,
}

/// Everything the invoice page, the PDF renderer and the backfill need about one
/// invoice. Public because the document backfill assembles the same payload offline.
pub async fn invoice_detail(conn: &mut PgConnection, id: Uuid) -> ApiResult<InvoiceDetail> {
    let summary: Option<InvoiceSummary> = sqlx::query_as(&format!("{INVOICE_SELECT} i.id = $1"))
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?;
    let summary = summary.ok_or_else(|| ApiError::not_found("invoice"))?;
    let extra: InvoiceExtra = sqlx::query_as(
        "select notes, created_by, approved_by, issued_by, journal_entry_id, pdf_s3_key
           from invoices where id = $1",
    )
    .bind(id)
    .fetch_one(&mut *conn)
    .await?;
    let customer: CustomerBlock = sqlx::query_as(
        "select id, code::text as code, name, contact_name, contact_email::text as contact_email,
                phone, billing_address, currency::text as currency, status
           from customers where id = $1",
    )
    .bind(summary.customer_id)
    .fetch_one(&mut *conn)
    .await?;
    let lines: Vec<InvoiceLineOut> = sqlx::query_as(
        "select id, invoice_id, seq, description, quantity, unit_price, tax_rate, amount
           from invoice_lines where invoice_id = $1 order by seq",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    let payments: Vec<PaymentOut> = sqlx::query_as(
        "select id, invoice_id, received_on, amount, method, reference, recorded_by,
                journal_entry_id, created_at
           from payments where invoice_id = $1 order by received_on, created_at",
    )
    .bind(id)
    .fetch_all(conn)
    .await?;
    Ok(InvoiceDetail {
        summary,
        notes: extra.notes,
        created_by: extra.created_by,
        approved_by: extra.approved_by,
        issued_by: extra.issued_by,
        journal_entry_id: extra.journal_entry_id,
        pdf_s3_key: extra.pdf_s3_key,
        customer,
        lines,
        payments,
    })
}

#[utoipa::path(get, path = "/api/v1/finance/invoices/{id}", tag = "finance", security(("bearer" = [])),
    responses((status = 200, body = InvoiceDetail), (status = 404, body = Problem)))]
pub async fn get_invoice(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<InvoiceDetail>> {
    actor.require_any(&["ledger:read", "customers:read"])?;
    let mut conn = state.pool.acquire().await?;
    Ok(Json(invoice_detail(&mut conn, id).await?))
}

#[derive(Debug, Deserialize, Validate, ToSchema, Clone)]
pub struct NewInvoiceLine {
    #[validate(length(min = 1, max = 300))]
    pub description: String,
    pub quantity: Decimal,
    pub unit_price: Decimal,
    /// Fraction between 0 and 1, for example `0.2` for twenty percent.
    pub tax_rate: Option<Decimal>,
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct NewInvoice {
    pub customer_id: Uuid,
    pub shipment_id: Option<Uuid>,
    #[validate(length(equal = 3))]
    pub currency: Option<String>,
    #[validate(range(min = 0, max = 365))]
    pub due_days: Option<i32>,
    #[validate(length(max = 2000))]
    pub notes: Option<String>,
    #[validate(nested)]
    pub lines: Vec<NewInvoiceLine>,
}

struct Totals {
    subtotal: Decimal,
    tax: Decimal,
    total: Decimal,
    amounts: Vec<Decimal>,
}

/// Line amount is quantity times unit price; tax is charged per line and summed, so a
/// mixed-rate invoice adds up exactly the way the printed document does.
fn totals(lines: &[NewInvoiceLine]) -> ApiResult<Totals> {
    if lines.is_empty() {
        return Err(ApiError::validation(
            "lines",
            "an invoice needs at least one line",
        ));
    }
    if lines.len() > MAX_LINES {
        return Err(ApiError::validation(
            "lines",
            format!("an invoice may carry at most {MAX_LINES} lines"),
        ));
    }
    let mut subtotal = Decimal::ZERO;
    let mut tax = Decimal::ZERO;
    let mut amounts = Vec::with_capacity(lines.len());
    for (idx, line) in lines.iter().enumerate() {
        if line.quantity <= Decimal::ZERO {
            return Err(ApiError::validation(
                format!("lines[{idx}].quantity"),
                "must be greater than zero",
            ));
        }
        if line.unit_price < Decimal::ZERO {
            return Err(ApiError::validation(
                format!("lines[{idx}].unit_price"),
                "must not be negative",
            ));
        }
        let rate = line.tax_rate.unwrap_or(Decimal::ZERO);
        if rate < Decimal::ZERO || rate > Decimal::ONE {
            return Err(ApiError::validation(
                format!("lines[{idx}].tax_rate"),
                "must be a fraction between 0 and 1",
            ));
        }
        let amount = (line.quantity * line.unit_price).round_dp(2);
        subtotal += amount;
        tax += (amount * rate).round_dp(2);
        amounts.push(amount);
    }
    Ok(Totals {
        subtotal,
        tax,
        total: subtotal + tax,
        amounts,
    })
}

async fn replace_lines(
    conn: &mut PgConnection,
    invoice_id: Uuid,
    lines: &[NewInvoiceLine],
    amounts: &[Decimal],
) -> ApiResult<()> {
    sqlx::query("delete from invoice_lines where invoice_id = $1")
        .bind(invoice_id)
        .execute(&mut *conn)
        .await?;
    for (idx, line) in lines.iter().enumerate() {
        sqlx::query(
            "insert into invoice_lines (invoice_id, seq, description, quantity, unit_price, tax_rate, amount)
             values ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(invoice_id)
        .bind((idx + 1) as i16)
        .bind(&line.description)
        .bind(line.quantity)
        .bind(line.unit_price.round_dp(2))
        .bind(line.tax_rate.unwrap_or(Decimal::ZERO))
        .bind(amounts[idx])
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

#[utoipa::path(post, path = "/api/v1/finance/invoices", tag = "finance", security(("bearer" = [])),
    request_body = NewInvoice,
    responses((status = 201, body = InvoiceDetail), (status = 422, body = Problem)))]
pub async fn create_invoice(
    State(state): State<AppState>,
    actor: Actor,
    ValidatedJson(body): ValidatedJson<NewInvoice>,
) -> ApiResult<(StatusCode, Json<InvoiceDetail>)> {
    actor.require("invoices:draft")?;
    let totals = totals(&body.lines)?;
    let currency = body
        .currency
        .unwrap_or_else(|| "USD".to_string())
        .to_uppercase();
    let due_days = body.due_days.unwrap_or(DEFAULT_DUE_DAYS);
    let mut tx = state.pool.begin().await?;
    let invoice_no = org::next_invoice_no(&mut tx).await?;
    // The draft carries its payment terms as a provisional due date; issuing rebases it
    // on the real issue date.
    let id: Uuid = sqlx::query_scalar(
        "insert into invoices (invoice_no, customer_id, shipment_id, currency, subtotal, tax, total,
                               notes, created_by, due_date)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, current_date + $10::int) returning id",
    )
    .bind(&invoice_no)
    .bind(body.customer_id)
    .bind(body.shipment_id)
    .bind(&currency)
    .bind(totals.subtotal)
    .bind(totals.tax)
    .bind(totals.total)
    .bind(&body.notes)
    .bind(actor.me())
    .bind(due_days)
    .fetch_one(&mut *tx)
    .await?;
    replace_lines(&mut tx, id, &body.lines, &totals.amounts).await?;
    let after = audit::snapshot(&mut tx, "invoices", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "invoice.create",
        "invoice",
        Some(id),
        None,
        after,
    )
    .await?;
    let detail = invoice_detail(&mut tx, id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(detail)))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct UpdateInvoice {
    pub customer_id: Option<Uuid>,
    pub shipment_id: Option<Uuid>,
    #[validate(length(equal = 3))]
    pub currency: Option<String>,
    #[validate(range(min = 0, max = 365))]
    pub due_days: Option<i32>,
    #[validate(length(max = 2000))]
    pub notes: Option<String>,
    /// When present the lines are replaced and the totals recomputed.
    #[validate(nested)]
    pub lines: Option<Vec<NewInvoiceLine>>,
}

#[utoipa::path(patch, path = "/api/v1/finance/invoices/{id}", tag = "finance", security(("bearer" = [])),
    request_body = UpdateInvoice,
    responses((status = 200, body = InvoiceDetail), (status = 409, body = Problem)))]
pub async fn update_invoice(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<UpdateInvoice>,
) -> ApiResult<Json<InvoiceDetail>> {
    actor.require("invoices:draft")?;
    let mut tx = state.pool.begin().await?;
    let status = lock_status(&mut tx, id).await?;
    if !EDITABLE.contains(&status.as_str()) {
        return Err(ApiError::InvalidTransition(format!(
            "only draft invoices can be edited, this one is {status}"
        )));
    }
    let before = audit::snapshot(&mut tx, "invoices", id).await?;
    let mut qb: QueryBuilder<Postgres> =
        QueryBuilder::new("update invoices set updated_at = now()");
    if let Some(v) = body.customer_id {
        qb.push(", customer_id = ").push_bind(v);
    }
    if let Some(v) = body.shipment_id {
        qb.push(", shipment_id = ").push_bind(v);
    }
    if let Some(v) = &body.currency {
        qb.push(", currency = ").push_bind(v.to_uppercase());
    }
    if let Some(v) = body.due_days {
        qb.push(", due_date = current_date + ").push_bind(v);
    }
    if let Some(v) = &body.notes {
        qb.push(", notes = ").push_bind(v.clone());
    }
    if let Some(lines) = &body.lines {
        let totals = totals(lines)?;
        qb.push(", subtotal = ")
            .push_bind(totals.subtotal)
            .push(", tax = ")
            .push_bind(totals.tax)
            .push(", total = ")
            .push_bind(totals.total);
        qb.push(" where id = ").push_bind(id);
        qb.build().execute(&mut *tx).await?;
        replace_lines(&mut tx, id, lines, &totals.amounts).await?;
    } else {
        qb.push(" where id = ").push_bind(id);
        qb.build().execute(&mut *tx).await?;
    }
    let after = audit::snapshot(&mut tx, "invoices", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "invoice.update",
        "invoice",
        Some(id),
        before,
        after,
    )
    .await?;
    let detail = invoice_detail(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(detail))
}

async fn lock_status(conn: &mut PgConnection, id: Uuid) -> ApiResult<String> {
    sqlx::query_scalar("select status from invoices where id = $1 for update")
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(|| ApiError::not_found("invoice"))
}

#[utoipa::path(post, path = "/api/v1/finance/invoices/{id}/submit", tag = "finance", security(("bearer" = [])),
    responses((status = 200, body = InvoiceDetail), (status = 409, body = Problem)))]
pub async fn submit_invoice(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<InvoiceDetail>> {
    actor.require("invoices:draft")?;
    let mut tx = state.pool.begin().await?;
    let status = lock_status(&mut tx, id).await?;
    if status != "draft" {
        return Err(ApiError::transition(&status, "pending_approval"));
    }
    let total: Decimal = sqlx::query_scalar("select total from invoices where id = $1")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    if total <= Decimal::ZERO {
        return Err(ApiError::validation(
            "lines",
            "an invoice needs a total above zero",
        ));
    }
    let next = if total >= state.config.invoice_approval_threshold {
        "pending_approval"
    } else {
        "approved"
    };
    let before = audit::snapshot(&mut tx, "invoices", id).await?;
    sqlx::query("update invoices set status = $2 where id = $1")
        .bind(id)
        .bind(next)
        .execute(&mut *tx)
        .await?;
    let after = audit::snapshot(&mut tx, "invoices", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "invoice.submit",
        "invoice",
        Some(id),
        before,
        after,
    )
    .await?;
    let detail = invoice_detail(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(detail))
}

#[utoipa::path(post, path = "/api/v1/finance/invoices/{id}/approve", tag = "finance", security(("bearer" = [])),
    responses((status = 200, body = InvoiceDetail), (status = 409, body = Problem)))]
pub async fn approve_invoice(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<InvoiceDetail>> {
    actor.require("invoices:approve")?;
    let mut tx = state.pool.begin().await?;
    let status = lock_status(&mut tx, id).await?;
    if status != "pending_approval" {
        return Err(ApiError::transition(&status, "approved"));
    }
    let before = audit::snapshot(&mut tx, "invoices", id).await?;
    sqlx::query("update invoices set status = 'approved', approved_by = $2 where id = $1")
        .bind(id)
        .bind(actor.me())
        .execute(&mut *tx)
        .await?;
    let after = audit::snapshot(&mut tx, "invoices", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "invoice.approve",
        "invoice",
        Some(id),
        before,
        after,
    )
    .await?;
    let detail = invoice_detail(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(detail))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct IssueInvoice {
    /// Defaults to today; must fall inside an open fiscal period.
    pub issue_date: Option<NaiveDate>,
    /// Defaults to the payment terms captured on the draft.
    #[validate(range(min = 0, max = 365))]
    pub due_days: Option<i32>,
}

#[utoipa::path(post, path = "/api/v1/finance/invoices/{id}/issue", tag = "finance", security(("bearer" = [])),
    request_body = IssueInvoice,
    responses((status = 200, body = InvoiceDetail), (status = 409, body = Problem),
              (status = 422, body = Problem)))]
pub async fn issue_invoice(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    body: Option<ValidatedJson<IssueInvoice>>,
) -> ApiResult<Json<InvoiceDetail>> {
    actor.require("invoices:issue")?;
    let body = body.map(|ValidatedJson(b)| b);
    let issue_date = body
        .as_ref()
        .and_then(|b| b.issue_date)
        .unwrap_or_else(|| Utc::now().date_naive());
    let mut tx = state.pool.begin().await?;
    let status = lock_status(&mut tx, id).await?;
    if status != "approved" {
        return Err(ApiError::transition(&status, "issued"));
    }
    let stored_days: Option<i32> =
        sqlx::query_scalar("select (due_date - created_at::date)::int from invoices where id = $1")
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
    let due_days = body
        .and_then(|b| b.due_days)
        .or(stored_days)
        .unwrap_or(DEFAULT_DUE_DAYS)
        .max(0);
    let detail = invoice_detail(&mut tx, id).await?;
    let mut lines = vec![PostingLine::debit(
        ledger::ACCOUNTS_RECEIVABLE,
        detail.summary.total,
        format!("Invoice {}", detail.summary.invoice_no),
    )];
    lines.push(PostingLine::credit(
        ledger::FREIGHT_REVENUE,
        detail.summary.subtotal,
        format!("Invoice {} revenue", detail.summary.invoice_no),
    ));
    if detail.summary.tax != Decimal::ZERO {
        lines.push(PostingLine::credit(
            ledger::TAXES_PAYABLE,
            detail.summary.tax,
            format!("Invoice {} tax", detail.summary.invoice_no),
        ));
    }
    let posting = Posting::new(
        issue_date,
        format!(
            "Invoice {} to {}",
            detail.summary.invoice_no, detail.summary.customer_name
        ),
        "invoice",
        Some(id),
    )
    .with_lines(lines);
    let entry = ledger::post(&mut tx, &actor.audit(), actor.me(), posting).await?;
    let before = audit::snapshot(&mut tx, "invoices", id).await?;
    sqlx::query(
        "update invoices set status = 'issued', issue_date = $2::date, due_date = $2::date + $3::int,
                issued_by = $4, journal_entry_id = $5
          where id = $1",
    )
    .bind(id)
    .bind(issue_date)
    .bind(due_days)
    .bind(actor.me())
    .bind(entry.id)
    .execute(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "invoices", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "invoice.issue",
        "invoice",
        Some(id),
        before,
        after,
    )
    .await?;
    let detail = invoice_detail(&mut tx, id).await?;
    tx.commit().await?;
    // The ledger posting is the part that has to be durable. A billing service that is
    // slow or down must not undo an issued invoice, so the PDF is rendered afterwards
    // and simply retried by re-issuing the request for the document.
    let detail = match render_pdf(&state, &detail).await {
        Ok(key) => InvoiceDetail {
            summary: InvoiceSummary {
                has_pdf: true,
                ..detail.summary
            },
            pdf_s3_key: Some(key),
            ..detail
        },
        Err(_) => detail,
    };
    Ok(Json(detail))
}

/// The body `POST /render/invoice` expects. Public so that the document backfill
/// sends billing exactly what the API sends it.
pub fn render_payload(detail: &InvoiceDetail) -> serde_json::Value {
    json!({
        "invoice": {
            "id": detail.summary.id,
            "invoice_no": detail.summary.invoice_no,
            "issue_date": detail.summary.issue_date,
            "due_date": detail.summary.due_date,
            "currency": detail.summary.currency,
            "subtotal": detail.summary.subtotal,
            "tax": detail.summary.tax,
            "total": detail.summary.total,
            "notes": detail.notes,
        },
        "customer": {
            "id": detail.customer.id,
            "code": detail.customer.code,
            "name": detail.customer.name,
            "contact_name": detail.customer.contact_name,
            "contact_email": detail.customer.contact_email,
            "phone": detail.customer.phone,
            "billing_address": detail.customer.billing_address,
        },
        "shipment": {
            "id": detail.summary.shipment_id,
            "reference": detail.summary.shipment_reference,
        },
        "lines": detail.lines.iter().map(|l| json!({
            "seq": l.seq,
            "description": l.description,
            "quantity": l.quantity,
            "unit_price": l.unit_price,
            "tax_rate": l.tax_rate,
            "amount": l.amount,
        })).collect::<Vec<_>>(),
    })
}

/// Renders the invoice through billing and records the key it was stored under.
/// The error is already logged; callers that must not fail because billing is down
/// (issuing an invoice) simply drop it.
async fn render_pdf(state: &AppState, detail: &InvoiceDetail) -> ApiResult<String> {
    let id = detail.summary.id;
    let rendered = state
        .billing
        .render_invoice(&render_payload(detail))
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, invoice_id = %id, "invoice PDF rendering failed");
            ApiError::conflict(
                "the invoice PDF could not be rendered because the billing service did not answer; try again shortly",
            )
        })?;
    sqlx::query("update invoices set pdf_s3_key = $2 where id = $1")
        .bind(id)
        .bind(&rendered.s3_key)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, invoice_id = %id, "storing the invoice PDF key failed");
            ApiError::from(e)
        })?;
    Ok(rendered.s3_key)
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct VoidInvoice {
    #[validate(length(min = 1, max = 400))]
    pub reason: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/finance/invoices/{id}/void", tag = "finance", security(("bearer" = [])),
    request_body = VoidInvoice,
    responses((status = 200, body = InvoiceDetail), (status = 409, body = Problem)))]
pub async fn void_invoice(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    body: Option<ValidatedJson<VoidInvoice>>,
) -> ApiResult<Json<InvoiceDetail>> {
    actor.require("invoices:approve")?;
    let reason = body.and_then(|ValidatedJson(b)| b.reason);
    let mut tx = state.pool.begin().await?;
    let status = lock_status(&mut tx, id).await?;
    if !["draft", "pending_approval", "approved", "issued"].contains(&status.as_str()) {
        return Err(ApiError::transition(&status, "void"));
    }
    let detail = invoice_detail(&mut tx, id).await?;
    if detail.summary.amount_paid > Decimal::ZERO {
        return Err(ApiError::conflict(
            "an invoice with payments against it cannot be voided; refund it instead",
        ));
    }
    if let Some(entry_id) = detail.journal_entry_id {
        ledger::reverse(
            &mut tx,
            &actor.audit(),
            actor.me(),
            entry_id,
            Utc::now().date_naive(),
            Some(format!("Void of invoice {}", detail.summary.invoice_no)),
        )
        .await?;
    }
    let before = audit::snapshot(&mut tx, "invoices", id).await?;
    // The schema insists on both dates outside the drafting statuses, so voiding an
    // invoice that was never issued stamps today rather than inventing a date.
    sqlx::query(
        "update invoices set status = 'void',
                issue_date = coalesce(issue_date, current_date),
                due_date = coalesce(due_date, current_date),
                notes = case when $2::text is null then notes
                             else coalesce(notes || E'\\n', '') || 'Voided: ' || $2 end
          where id = $1",
    )
    .bind(id)
    .bind(&reason)
    .execute(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "invoices", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "invoice.void",
        "invoice",
        Some(id),
        before,
        after,
    )
    .await?;
    let detail = invoice_detail(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(detail))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DownloadUrl {
    pub url: String,
}

#[utoipa::path(get, path = "/api/v1/finance/invoices/{id}/pdf", tag = "finance", security(("bearer" = [])),
    responses((status = 200, body = DownloadUrl), (status = 409, body = Problem),
              (status = 404, body = Problem)))]
pub async fn invoice_pdf(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<DownloadUrl>> {
    actor.require_any(&["ledger:read", "customers:read"])?;
    let mut conn = state.pool.acquire().await?;
    let detail = invoice_detail(&mut conn, id).await?;
    drop(conn);
    let present = match &detail.pdf_s3_key {
        Some(key) => state
            .s3
            .object_exists(&state.s3.bucket_pdfs, key)
            .await?
            .then(|| key.clone()),
        None => None,
    };
    // An invoice PDF is a rendering of ledger data, not a file somebody uploaded, so a
    // key that points at nothing is repairable: render it again rather than hand the
    // caller a URL that answers 404.
    let key = match present {
        Some(key) => key,
        None => {
            if detail.summary.issue_date.is_none() || detail.summary.due_date.is_none() {
                return Err(ApiError::conflict(
                    "this invoice has no PDF yet; issue it and the document is rendered",
                ));
            }
            tracing::info!(invoice_id = %id, "invoice PDF is missing from storage; rendering it again");
            render_pdf(&state, &detail).await?
        }
    };
    let url = state.s3.presign_get(&state.s3.bucket_pdfs, &key).await?;
    Ok(Json(DownloadUrl { url }))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct NewPayment {
    pub invoice_id: Uuid,
    pub received_on: NaiveDate,
    pub amount: Decimal,
    /// `bank_transfer`, `card`, `cash` or `cheque`.
    pub method: String,
    #[validate(length(max = 120))]
    pub reference: Option<String>,
}

const PAYMENT_METHODS: &[&str] = &["bank_transfer", "card", "cash", "cheque"];

#[utoipa::path(post, path = "/api/v1/finance/payments", tag = "finance", security(("bearer" = [])),
    request_body = NewPayment,
    responses((status = 201, body = InvoiceDetail), (status = 409, body = Problem),
              (status = 422, body = Problem)))]
pub async fn record_payment(
    State(state): State<AppState>,
    actor: Actor,
    ValidatedJson(body): ValidatedJson<NewPayment>,
) -> ApiResult<(StatusCode, Json<InvoiceDetail>)> {
    actor.require("payments:record")?;
    if !PAYMENT_METHODS.contains(&body.method.as_str()) {
        return Err(ApiError::validation(
            "method",
            "must be bank_transfer, card, cash or cheque",
        ));
    }
    let amount = body.amount.round_dp(2);
    if amount <= Decimal::ZERO {
        return Err(ApiError::validation("amount", "must be greater than zero"));
    }
    let mut tx = state.pool.begin().await?;
    let status = lock_status(&mut tx, body.invoice_id).await?;
    if !["issued", "partially_paid"].contains(&status.as_str()) {
        return Err(ApiError::InvalidTransition(format!(
            "payments can only be recorded against issued invoices, this one is {status}"
        )));
    }
    let detail = invoice_detail(&mut tx, body.invoice_id).await?;
    let outstanding = detail.summary.total - detail.summary.amount_paid;
    if amount > outstanding {
        return Err(ApiError::validation(
            "amount",
            format!("exceeds the outstanding balance of {outstanding}"),
        ));
    }
    let posting = Posting::new(
        body.received_on,
        format!(
            "Payment for invoice {} from {}",
            detail.summary.invoice_no, detail.summary.customer_name
        ),
        "payment",
        Some(body.invoice_id),
    )
    .with_lines(vec![
        PostingLine::debit(
            ledger::CASH,
            amount,
            format!("Payment {}", detail.summary.invoice_no),
        ),
        PostingLine::credit(
            ledger::ACCOUNTS_RECEIVABLE,
            amount,
            format!("Payment {}", detail.summary.invoice_no),
        ),
    ]);
    let entry = ledger::post(&mut tx, &actor.audit(), actor.me(), posting).await?;
    let payment_id: Uuid = sqlx::query_scalar(
        "insert into payments (invoice_id, received_on, amount, method, reference, recorded_by,
                               journal_entry_id)
         values ($1, $2, $3, $4, $5, $6, $7) returning id",
    )
    .bind(body.invoice_id)
    .bind(body.received_on)
    .bind(amount)
    .bind(&body.method)
    .bind(&body.reference)
    .bind(actor.me())
    .bind(entry.id)
    .fetch_one(&mut *tx)
    .await?;
    let before = audit::snapshot(&mut tx, "invoices", body.invoice_id).await?;
    let paid_in_full = detail.summary.amount_paid + amount >= detail.summary.total;
    sqlx::query("update invoices set amount_paid = amount_paid + $2, status = $3 where id = $1")
        .bind(body.invoice_id)
        .bind(amount)
        .bind(if paid_in_full {
            "paid"
        } else {
            "partially_paid"
        })
        .execute(&mut *tx)
        .await?;
    let after = audit::snapshot(&mut tx, "invoices", body.invoice_id).await?;
    let payment = audit::snapshot(&mut tx, "payments", payment_id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "payment.record",
        "payment",
        Some(payment_id),
        before,
        payment,
    )
    .await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "invoice.payment",
        "invoice",
        Some(body.invoice_id),
        None,
        after,
    )
    .await?;
    let detail = invoice_detail(&mut tx, body.invoice_id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(detail)))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PaymentFilter {
    pub invoice_id: Option<Uuid>,
    pub from: Option<NaiveDate>,
    pub to: Option<NaiveDate>,
}

#[utoipa::path(get, path = "/api/v1/finance/payments", tag = "finance", security(("bearer" = [])),
    params(PageQuery, PaymentFilter),
    responses((status = 200, body = PageOut<PaymentOut>), (status = 403, body = Problem)))]
pub async fn list_payments(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<PaymentFilter>,
) -> ApiResult<Json<PageOut<PaymentOut>>> {
    actor.require_any(&["ledger:read", "payments:record"])?;
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        "select p.id, p.invoice_id, p.received_on, p.amount, p.method, p.reference, p.recorded_by,
                p.journal_entry_id, p.created_at, count(*) over() as total_count
           from payments p where true ",
    );
    if let Some(invoice) = filter.invoice_id {
        qb.push(" and p.invoice_id = ").push_bind(invoice);
    }
    if let Some(from) = filter.from {
        qb.push(" and p.received_on >= ").push_bind(from);
    }
    if let Some(to) = filter.to {
        qb.push(" and p.received_on <= ").push_bind(to);
    }
    let order = page.order_by(&[("received_on", "p.received_on"), ("amount", "p.amount")]);
    qb.push(format!(" order by {order} limit "));
    let paging = page.page();
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&state.pool).await?;
    let (items, total) = split_total::<PaymentOut>(rows)?;
    Ok(Json(PageOut::new(items, paging, total)))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/finance/invoices", get(list_invoices).post(create_invoice))
        .route(
            "/finance/invoices/:id",
            get(get_invoice).patch(update_invoice),
        )
        .route("/finance/invoices/:id/submit", post(submit_invoice))
        .route("/finance/invoices/:id/approve", post(approve_invoice))
        .route("/finance/invoices/:id/issue", post(issue_invoice))
        .route("/finance/invoices/:id/void", post(void_invoice))
        .route("/finance/invoices/:id/pdf", get(invoice_pdf))
        .route("/finance/payments", get(list_payments).post(record_payment))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(qty: &str, price: &str, rate: Option<&str>) -> NewInvoiceLine {
        NewInvoiceLine {
            description: "freight".to_string(),
            quantity: qty.parse().unwrap(),
            unit_price: price.parse().unwrap(),
            tax_rate: rate.map(|r| r.parse().unwrap()),
        }
    }

    #[test]
    fn totals_add_tax_per_line() {
        let t = totals(&[line("2", "100.00", Some("0.2")), line("1", "50.00", None)]).unwrap();
        assert_eq!(t.subtotal.to_string(), "250.00");
        assert_eq!(t.tax.to_string(), "40.00");
        assert_eq!(t.total.to_string(), "290.00");
        assert_eq!(t.amounts.len(), 2);
    }

    #[test]
    fn quantities_must_be_positive() {
        assert!(totals(&[line("0", "100.00", None)]).is_err());
    }

    #[test]
    fn tax_rates_are_fractions() {
        assert!(totals(&[line("1", "100.00", Some("20"))]).is_err());
    }
}
