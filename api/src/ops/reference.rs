//! Operational reference data: customers, carriers, sites and vehicles.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgConnection, Postgres, QueryBuilder};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;
use validator::Validate;

use crate::audit;
use crate::auth::Actor;
use crate::error::{ApiError, ApiResult, Problem};
use crate::http::extract::ValidatedJson;
use crate::http::pagination::{split_total, PageOut, PageQuery};
use crate::ops::service;
use crate::state::AppState;

fn currency_or_default(currency: Option<&str>) -> ApiResult<String> {
    match currency {
        None => Ok("USD".to_string()),
        Some(code) if code.len() == 3 && code.chars().all(|c| c.is_ascii_alphabetic()) => {
            Ok(code.to_ascii_uppercase())
        }
        Some(_) => Err(ApiError::validation(
            "currency",
            "must be a three letter code",
        )),
    }
}

fn check_not_negative(field: &'static str, value: Option<Decimal>) -> ApiResult<()> {
    if value.is_some_and(|v| v < Decimal::ZERO) {
        return Err(ApiError::validation(field, "must not be negative"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Customers
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct CustomerOut {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub contact_name: Option<String>,
    pub contact_email: Option<String>,
    pub phone: Option<String>,
    pub billing_address: Value,
    pub credit_limit: Decimal,
    pub currency: String,
    pub status: String,
    pub account_manager_id: Option<Uuid>,
    pub account_manager_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const CUSTOMER_SELECT: &str =
    "select c.id, c.code::text as code, c.name, c.contact_name, c.contact_email::text as contact_email,
            c.phone, c.billing_address, c.credit_limit, c.currency, c.status, c.account_manager_id,
            m.first_name || ' ' || m.last_name as account_manager_name, c.created_at, c.updated_at,
            count(*) over() as total_count
       from customers c left join employees m on m.id = c.account_manager_id
      where ";

async fn fetch_customer(conn: &mut PgConnection, id: Uuid) -> ApiResult<CustomerOut> {
    sqlx::query_as(&format!("{CUSTOMER_SELECT} c.id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(|| ApiError::not_found("customer"))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CustomerFilter {
    /// `active`, `on_hold` or `closed`.
    pub status: Option<String>,
    pub account_manager_id: Option<Uuid>,
}

#[utoipa::path(get, path = "/api/v1/ops/customers", tag = "ops", security(("bearer" = [])),
    params(PageQuery, CustomerFilter), responses((status = 200, body = PageOut<CustomerOut>)))]
pub async fn list_customers(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<CustomerFilter>,
) -> ApiResult<Json<PageOut<CustomerOut>>> {
    actor.require("customers:read")?;
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(CUSTOMER_SELECT);
    qb.push(" true ");
    if let Some(status) = &filter.status {
        service::check_one_of("status", status, &service::CUSTOMER_STATUSES)?;
        qb.push(" and c.status = ").push_bind(status.clone());
    }
    if let Some(manager) = filter.account_manager_id {
        qb.push(" and c.account_manager_id = ").push_bind(manager);
    }
    if let Some(q) = page.search() {
        qb.push(" and (c.name ilike ")
            .push_bind(q.clone())
            .push(" or c.code::text ilike ")
            .push_bind(q.clone())
            .push(" or c.contact_name ilike ")
            .push_bind(q)
            .push(")");
    }
    let order = page.order_by(&[
        ("name", "c.name"),
        ("code", "c.code"),
        ("created_at", "c.created_at"),
        ("credit_limit", "c.credit_limit"),
    ]);
    let order = if page.sort.is_none() {
        "c.name asc".to_string()
    } else {
        order
    };
    let paging = page.page();
    qb.push(format!(" order by {order} limit "));
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&state.pool).await?;
    let (items, total) = split_total::<CustomerOut>(rows)?;
    Ok(Json(PageOut::new(items, paging, total)))
}

#[utoipa::path(get, path = "/api/v1/ops/customers/{id}", tag = "ops", security(("bearer" = [])),
    responses((status = 200, body = CustomerOut), (status = 404, body = Problem)))]
pub async fn get_customer(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<CustomerOut>> {
    actor.require("customers:read")?;
    let mut conn = state.pool.acquire().await?;
    Ok(Json(fetch_customer(&mut conn, id).await?))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateCustomer {
    #[validate(length(min = 1, max = 20))]
    pub code: String,
    #[validate(length(min = 1, max = 200))]
    pub name: String,
    #[validate(length(max = 120))]
    pub contact_name: Option<String>,
    #[validate(email)]
    pub contact_email: Option<String>,
    #[validate(length(max = 40))]
    pub phone: Option<String>,
    pub billing_address: Option<Value>,
    pub credit_limit: Option<Decimal>,
    pub currency: Option<String>,
    pub account_manager_id: Option<Uuid>,
}

#[utoipa::path(post, path = "/api/v1/ops/customers", tag = "ops", security(("bearer" = [])),
    request_body = CreateCustomer,
    responses((status = 201, body = CustomerOut), (status = 409, body = Problem)))]
pub async fn create_customer(
    State(state): State<AppState>,
    actor: Actor,
    ValidatedJson(body): ValidatedJson<CreateCustomer>,
) -> ApiResult<(StatusCode, Json<CustomerOut>)> {
    actor.require("customers:manage")?;
    let currency = currency_or_default(body.currency.as_deref())?;
    check_not_negative("credit_limit", body.credit_limit)?;
    let billing_address = body.billing_address.clone().unwrap_or_else(|| json!({}));
    service::check_json_object("billing_address", &billing_address)?;
    let mut tx = state.pool.begin().await?;
    let id: Uuid = sqlx::query_scalar(
        "insert into customers (code, name, contact_name, contact_email, phone, billing_address,
                                credit_limit, currency, account_manager_id)
         values ($1::citext, $2, $3, $4::citext, $5, $6, $7, $8, $9) returning id",
    )
    .bind(&body.code)
    .bind(&body.name)
    .bind(&body.contact_name)
    .bind(&body.contact_email)
    .bind(&body.phone)
    .bind(&billing_address)
    .bind(body.credit_limit.unwrap_or(Decimal::ZERO))
    .bind(&currency)
    .bind(body.account_manager_id)
    .fetch_one(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "customers", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "customer.create",
        "customer",
        Some(id),
        None,
        after,
    )
    .await?;
    let out = fetch_customer(&mut tx, id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(out)))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct UpdateCustomer {
    #[validate(length(min = 1, max = 200))]
    pub name: Option<String>,
    #[validate(length(max = 120))]
    pub contact_name: Option<String>,
    #[validate(email)]
    pub contact_email: Option<String>,
    #[validate(length(max = 40))]
    pub phone: Option<String>,
    pub billing_address: Option<Value>,
    pub credit_limit: Option<Decimal>,
    pub currency: Option<String>,
    /// `active`, `on_hold` or `closed`.
    pub status: Option<String>,
    pub account_manager_id: Option<Uuid>,
}

#[utoipa::path(patch, path = "/api/v1/ops/customers/{id}", tag = "ops", security(("bearer" = [])),
    request_body = UpdateCustomer,
    responses((status = 200, body = CustomerOut), (status = 404, body = Problem)))]
pub async fn update_customer(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<UpdateCustomer>,
) -> ApiResult<Json<CustomerOut>> {
    actor.require("customers:manage")?;
    check_not_negative("credit_limit", body.credit_limit)?;
    if let Some(status) = &body.status {
        service::check_one_of("status", status, &service::CUSTOMER_STATUSES)?;
    }
    if let Some(address) = &body.billing_address {
        service::check_json_object("billing_address", address)?;
    }
    let mut tx = state.pool.begin().await?;
    let before = audit::snapshot(&mut tx, "customers", id)
        .await?
        .ok_or_else(|| ApiError::not_found("customer"))?;
    let mut qb: QueryBuilder<Postgres> =
        QueryBuilder::new("update customers set updated_at = now()");
    if let Some(v) = &body.name {
        qb.push(", name = ").push_bind(v.clone());
    }
    if let Some(v) = &body.contact_name {
        qb.push(", contact_name = ").push_bind(v.clone());
    }
    if let Some(v) = &body.contact_email {
        qb.push(", contact_email = ")
            .push_bind(v.clone())
            .push("::citext");
    }
    if let Some(v) = &body.phone {
        qb.push(", phone = ").push_bind(v.clone());
    }
    if let Some(v) = &body.billing_address {
        qb.push(", billing_address = ").push_bind(v.clone());
    }
    if let Some(v) = body.credit_limit {
        qb.push(", credit_limit = ").push_bind(v);
    }
    if let Some(v) = &body.currency {
        qb.push(", currency = ")
            .push_bind(currency_or_default(Some(v))?);
    }
    if let Some(v) = &body.status {
        qb.push(", status = ").push_bind(v.clone());
    }
    if let Some(v) = body.account_manager_id {
        qb.push(", account_manager_id = ").push_bind(v);
    }
    qb.push(" where id = ").push_bind(id);
    qb.build().execute(&mut *tx).await?;
    let after = audit::snapshot(&mut tx, "customers", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "customer.update",
        "customer",
        Some(id),
        Some(before),
        after,
    )
    .await?;
    let out = fetch_customer(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(out))
}

// ---------------------------------------------------------------------------
// Carriers
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct CarrierOut {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub mode: String,
    pub scac: Option<String>,
    pub contact: Value,
    pub on_time_rate: Option<Decimal>,
    pub active: bool,
}

const CARRIER_SELECT: &str =
    "select c.id, c.code::text as code, c.name, c.mode, c.scac, c.contact, c.on_time_rate, c.active,
            count(*) over() as total_count
       from carriers c
      where ";

async fn fetch_carrier(conn: &mut PgConnection, id: Uuid) -> ApiResult<CarrierOut> {
    sqlx::query_as(&format!("{CARRIER_SELECT} c.id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(|| ApiError::not_found("carrier"))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CarrierFilter {
    /// `sea`, `air`, `road` or `rail`.
    pub mode: Option<String>,
    /// `1` to hide retired carriers.
    pub active: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/ops/carriers", tag = "ops", security(("bearer" = [])),
    params(PageQuery, CarrierFilter), responses((status = 200, body = PageOut<CarrierOut>)))]
pub async fn list_carriers(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<CarrierFilter>,
) -> ApiResult<Json<PageOut<CarrierOut>>> {
    actor.require("shipments:read")?;
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(CARRIER_SELECT);
    qb.push(" true ");
    if let Some(mode) = &filter.mode {
        service::check_one_of("mode", mode, &service::MODES)?;
        qb.push(" and c.mode = ").push_bind(mode.clone());
    }
    if service::truthy(filter.active.as_deref()) {
        qb.push(" and c.active");
    }
    if let Some(q) = page.search() {
        qb.push(" and (c.name ilike ")
            .push_bind(q.clone())
            .push(" or c.code::text ilike ")
            .push_bind(q.clone())
            .push(" or c.scac ilike ")
            .push_bind(q)
            .push(")");
    }
    let order = page.order_by(&[
        ("name", "c.name"),
        ("code", "c.code"),
        ("on_time_rate", "c.on_time_rate"),
    ]);
    let order = if page.sort.is_none() {
        "c.name asc".to_string()
    } else {
        order
    };
    let paging = page.page();
    qb.push(format!(" order by {order} limit "));
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&state.pool).await?;
    let (items, total) = split_total::<CarrierOut>(rows)?;
    Ok(Json(PageOut::new(items, paging, total)))
}

#[utoipa::path(get, path = "/api/v1/ops/carriers/{id}", tag = "ops", security(("bearer" = [])),
    responses((status = 200, body = CarrierOut), (status = 404, body = Problem)))]
pub async fn get_carrier(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<CarrierOut>> {
    actor.require("shipments:read")?;
    let mut conn = state.pool.acquire().await?;
    Ok(Json(fetch_carrier(&mut conn, id).await?))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateCarrier {
    #[validate(length(min = 1, max = 20))]
    pub code: String,
    #[validate(length(min = 1, max = 200))]
    pub name: String,
    /// `sea`, `air`, `road` or `rail`.
    pub mode: String,
    #[validate(length(max = 10))]
    pub scac: Option<String>,
    pub contact: Option<Value>,
    /// Share of on-time arrivals, 0 to 1.
    pub on_time_rate: Option<Decimal>,
    pub active: Option<bool>,
}

fn check_rate(value: Option<Decimal>) -> ApiResult<()> {
    if value.is_some_and(|v| v < Decimal::ZERO || v > Decimal::ONE) {
        return Err(ApiError::validation(
            "on_time_rate",
            "must be between 0 and 1",
        ));
    }
    Ok(())
}

#[utoipa::path(post, path = "/api/v1/ops/carriers", tag = "ops", security(("bearer" = [])),
    request_body = CreateCarrier,
    responses((status = 201, body = CarrierOut), (status = 409, body = Problem)))]
pub async fn create_carrier(
    State(state): State<AppState>,
    actor: Actor,
    ValidatedJson(body): ValidatedJson<CreateCarrier>,
) -> ApiResult<(StatusCode, Json<CarrierOut>)> {
    actor.require("fleet:manage")?;
    service::check_one_of("mode", &body.mode, &service::MODES)?;
    check_rate(body.on_time_rate)?;
    let contact = body.contact.clone().unwrap_or_else(|| json!({}));
    service::check_json_object("contact", &contact)?;
    let mut tx = state.pool.begin().await?;
    let id: Uuid = sqlx::query_scalar(
        "insert into carriers (code, name, mode, scac, contact, on_time_rate, active)
         values ($1::citext, $2, $3, $4, $5, $6, $7) returning id",
    )
    .bind(&body.code)
    .bind(&body.name)
    .bind(&body.mode)
    .bind(&body.scac)
    .bind(&contact)
    .bind(body.on_time_rate)
    .bind(body.active.unwrap_or(true))
    .fetch_one(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "carriers", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "carrier.create",
        "carrier",
        Some(id),
        None,
        after,
    )
    .await?;
    let out = fetch_carrier(&mut tx, id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(out)))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct UpdateCarrier {
    #[validate(length(min = 1, max = 200))]
    pub name: Option<String>,
    pub mode: Option<String>,
    #[validate(length(max = 10))]
    pub scac: Option<String>,
    pub contact: Option<Value>,
    pub on_time_rate: Option<Decimal>,
    pub active: Option<bool>,
}

#[utoipa::path(patch, path = "/api/v1/ops/carriers/{id}", tag = "ops", security(("bearer" = [])),
    request_body = UpdateCarrier,
    responses((status = 200, body = CarrierOut), (status = 404, body = Problem)))]
pub async fn update_carrier(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<UpdateCarrier>,
) -> ApiResult<Json<CarrierOut>> {
    actor.require("fleet:manage")?;
    check_rate(body.on_time_rate)?;
    if let Some(mode) = &body.mode {
        service::check_one_of("mode", mode, &service::MODES)?;
    }
    if let Some(contact) = &body.contact {
        service::check_json_object("contact", contact)?;
    }
    let mut tx = state.pool.begin().await?;
    let before = audit::snapshot(&mut tx, "carriers", id)
        .await?
        .ok_or_else(|| ApiError::not_found("carrier"))?;
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("update carriers set ");
    let mut sets = qb.separated(", ");
    let mut changed = false;
    if let Some(v) = &body.name {
        sets.push("name = ").push_bind_unseparated(v.clone());
        changed = true;
    }
    if let Some(v) = &body.mode {
        sets.push("mode = ").push_bind_unseparated(v.clone());
        changed = true;
    }
    if let Some(v) = &body.scac {
        sets.push("scac = ").push_bind_unseparated(v.clone());
        changed = true;
    }
    if let Some(v) = &body.contact {
        sets.push("contact = ").push_bind_unseparated(v.clone());
        changed = true;
    }
    if let Some(v) = body.on_time_rate {
        sets.push("on_time_rate = ").push_bind_unseparated(v);
        changed = true;
    }
    if let Some(v) = body.active {
        sets.push("active = ").push_bind_unseparated(v);
        changed = true;
    }
    if !changed {
        return Err(ApiError::validation("body", "no fields to update"));
    }
    qb.push(" where id = ").push_bind(id);
    qb.build().execute(&mut *tx).await?;
    let after = audit::snapshot(&mut tx, "carriers", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "carrier.update",
        "carrier",
        Some(id),
        Some(before),
        after,
    )
    .await?;
    let out = fetch_carrier(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(out))
}

// ---------------------------------------------------------------------------
// Sites
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct SiteOut {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub kind: String,
    pub address: Value,
    pub manager_id: Option<Uuid>,
    pub manager_name: Option<String>,
}

const SITE_SELECT: &str =
    "select s.id, s.code::text as code, s.name, s.kind, s.address, s.manager_id,
            m.first_name || ' ' || m.last_name as manager_name, count(*) over() as total_count
       from sites s left join employees m on m.id = s.manager_id
      where ";

async fn fetch_site(conn: &mut PgConnection, id: Uuid) -> ApiResult<SiteOut> {
    sqlx::query_as(&format!("{SITE_SELECT} s.id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(|| ApiError::not_found("site"))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct SiteFilter {
    /// `office`, `warehouse`, `port`, `airport` or `depot`.
    pub kind: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/ops/sites", tag = "ops", security(("bearer" = [])),
    params(PageQuery, SiteFilter), responses((status = 200, body = PageOut<SiteOut>)))]
pub async fn list_sites(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<SiteFilter>,
) -> ApiResult<Json<PageOut<SiteOut>>> {
    actor.require("shipments:read")?;
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(SITE_SELECT);
    qb.push(" true ");
    if let Some(kind) = &filter.kind {
        service::check_one_of("kind", kind, &service::SITE_KINDS)?;
        qb.push(" and s.kind = ").push_bind(kind.clone());
    }
    if let Some(q) = page.search() {
        qb.push(" and (s.name ilike ")
            .push_bind(q.clone())
            .push(" or s.code::text ilike ")
            .push_bind(q)
            .push(")");
    }
    let paging = page.page();
    qb.push(" order by s.name asc limit ");
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&state.pool).await?;
    let (items, total) = split_total::<SiteOut>(rows)?;
    Ok(Json(PageOut::new(items, paging, total)))
}

#[utoipa::path(get, path = "/api/v1/ops/sites/{id}", tag = "ops", security(("bearer" = [])),
    responses((status = 200, body = SiteOut), (status = 404, body = Problem)))]
pub async fn get_site(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<SiteOut>> {
    actor.require("shipments:read")?;
    let mut conn = state.pool.acquire().await?;
    Ok(Json(fetch_site(&mut conn, id).await?))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateSite {
    #[validate(length(min = 1, max = 20))]
    pub code: String,
    #[validate(length(min = 1, max = 200))]
    pub name: String,
    /// `office`, `warehouse`, `port`, `airport` or `depot`.
    pub kind: String,
    pub address: Option<Value>,
    pub manager_id: Option<Uuid>,
}

#[utoipa::path(post, path = "/api/v1/ops/sites", tag = "ops", security(("bearer" = [])),
    request_body = CreateSite, responses((status = 201, body = SiteOut), (status = 409, body = Problem)))]
pub async fn create_site(
    State(state): State<AppState>,
    actor: Actor,
    ValidatedJson(body): ValidatedJson<CreateSite>,
) -> ApiResult<(StatusCode, Json<SiteOut>)> {
    actor.require("fleet:manage")?;
    service::check_one_of("kind", &body.kind, &service::SITE_KINDS)?;
    let address = body.address.clone().unwrap_or_else(|| json!({}));
    service::check_json_object("address", &address)?;
    let mut tx = state.pool.begin().await?;
    let id: Uuid = sqlx::query_scalar(
        "insert into sites (code, name, kind, address, manager_id)
         values ($1::citext, $2, $3, $4, $5) returning id",
    )
    .bind(&body.code)
    .bind(&body.name)
    .bind(&body.kind)
    .bind(&address)
    .bind(body.manager_id)
    .fetch_one(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "sites", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "site.create",
        "site",
        Some(id),
        None,
        after,
    )
    .await?;
    let out = fetch_site(&mut tx, id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(out)))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct UpdateSite {
    #[validate(length(min = 1, max = 200))]
    pub name: Option<String>,
    pub kind: Option<String>,
    pub address: Option<Value>,
    pub manager_id: Option<Uuid>,
}

#[utoipa::path(patch, path = "/api/v1/ops/sites/{id}", tag = "ops", security(("bearer" = [])),
    request_body = UpdateSite, responses((status = 200, body = SiteOut), (status = 404, body = Problem)))]
pub async fn update_site(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<UpdateSite>,
) -> ApiResult<Json<SiteOut>> {
    actor.require("fleet:manage")?;
    if let Some(kind) = &body.kind {
        service::check_one_of("kind", kind, &service::SITE_KINDS)?;
    }
    if let Some(address) = &body.address {
        service::check_json_object("address", address)?;
    }
    let mut tx = state.pool.begin().await?;
    let before = audit::snapshot(&mut tx, "sites", id)
        .await?
        .ok_or_else(|| ApiError::not_found("site"))?;
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("update sites set ");
    let mut sets = qb.separated(", ");
    let mut changed = false;
    if let Some(v) = &body.name {
        sets.push("name = ").push_bind_unseparated(v.clone());
        changed = true;
    }
    if let Some(v) = &body.kind {
        sets.push("kind = ").push_bind_unseparated(v.clone());
        changed = true;
    }
    if let Some(v) = &body.address {
        sets.push("address = ").push_bind_unseparated(v.clone());
        changed = true;
    }
    if let Some(v) = body.manager_id {
        sets.push("manager_id = ").push_bind_unseparated(v);
        changed = true;
    }
    if !changed {
        return Err(ApiError::validation("body", "no fields to update"));
    }
    qb.push(" where id = ").push_bind(id);
    qb.build().execute(&mut *tx).await?;
    let after = audit::snapshot(&mut tx, "sites", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "site.update",
        "site",
        Some(id),
        Some(before),
        after,
    )
    .await?;
    let out = fetch_site(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(out))
}

// ---------------------------------------------------------------------------
// Vehicles
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct VehicleOut {
    pub id: Uuid,
    pub plate: String,
    pub kind: String,
    pub capacity_kg: Option<Decimal>,
    pub status: String,
    pub home_site_id: Option<Uuid>,
    pub home_site_name: Option<String>,
}

const VEHICLE_SELECT: &str =
    "select v.id, v.plate::text as plate, v.kind, v.capacity_kg, v.status, v.home_site_id,
            s.name as home_site_name, count(*) over() as total_count
       from vehicles v left join sites s on s.id = v.home_site_id
      where ";

async fn fetch_vehicle(conn: &mut PgConnection, id: Uuid) -> ApiResult<VehicleOut> {
    sqlx::query_as(&format!("{VEHICLE_SELECT} v.id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(|| ApiError::not_found("vehicle"))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct VehicleFilter {
    /// `truck`, `van`, `trailer` or `forklift`.
    pub kind: Option<String>,
    /// `available`, `in_use`, `maintenance` or `retired`.
    pub status: Option<String>,
    pub home_site_id: Option<Uuid>,
}

#[utoipa::path(get, path = "/api/v1/ops/vehicles", tag = "ops", security(("bearer" = [])),
    params(PageQuery, VehicleFilter), responses((status = 200, body = PageOut<VehicleOut>)))]
pub async fn list_vehicles(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<VehicleFilter>,
) -> ApiResult<Json<PageOut<VehicleOut>>> {
    actor.require("shipments:read")?;
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(VEHICLE_SELECT);
    qb.push(" true ");
    if let Some(kind) = &filter.kind {
        service::check_one_of("kind", kind, &service::VEHICLE_KINDS)?;
        qb.push(" and v.kind = ").push_bind(kind.clone());
    }
    if let Some(status) = &filter.status {
        service::check_one_of("status", status, &service::VEHICLE_STATUSES)?;
        qb.push(" and v.status = ").push_bind(status.clone());
    }
    if let Some(site) = filter.home_site_id {
        qb.push(" and v.home_site_id = ").push_bind(site);
    }
    if let Some(q) = page.search() {
        qb.push(" and v.plate::text ilike ").push_bind(q);
    }
    let paging = page.page();
    qb.push(" order by v.plate asc limit ");
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&state.pool).await?;
    let (items, total) = split_total::<VehicleOut>(rows)?;
    Ok(Json(PageOut::new(items, paging, total)))
}

#[utoipa::path(get, path = "/api/v1/ops/vehicles/{id}", tag = "ops", security(("bearer" = [])),
    responses((status = 200, body = VehicleOut), (status = 404, body = Problem)))]
pub async fn get_vehicle(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<VehicleOut>> {
    actor.require("shipments:read")?;
    let mut conn = state.pool.acquire().await?;
    Ok(Json(fetch_vehicle(&mut conn, id).await?))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateVehicle {
    #[validate(length(min = 1, max = 20))]
    pub plate: String,
    /// `truck`, `van`, `trailer` or `forklift`.
    pub kind: String,
    pub capacity_kg: Option<Decimal>,
    pub status: Option<String>,
    pub home_site_id: Option<Uuid>,
}

#[utoipa::path(post, path = "/api/v1/ops/vehicles", tag = "ops", security(("bearer" = [])),
    request_body = CreateVehicle,
    responses((status = 201, body = VehicleOut), (status = 409, body = Problem)))]
pub async fn create_vehicle(
    State(state): State<AppState>,
    actor: Actor,
    ValidatedJson(body): ValidatedJson<CreateVehicle>,
) -> ApiResult<(StatusCode, Json<VehicleOut>)> {
    actor.require("fleet:manage")?;
    service::check_one_of("kind", &body.kind, &service::VEHICLE_KINDS)?;
    let status = body
        .status
        .clone()
        .unwrap_or_else(|| "available".to_string());
    service::check_one_of("status", &status, &service::VEHICLE_STATUSES)?;
    check_not_negative("capacity_kg", body.capacity_kg)?;
    let mut tx = state.pool.begin().await?;
    let id: Uuid = sqlx::query_scalar(
        "insert into vehicles (plate, kind, capacity_kg, status, home_site_id)
         values ($1::citext, $2, $3, $4, $5) returning id",
    )
    .bind(&body.plate)
    .bind(&body.kind)
    .bind(body.capacity_kg)
    .bind(&status)
    .bind(body.home_site_id)
    .fetch_one(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "vehicles", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "vehicle.create",
        "vehicle",
        Some(id),
        None,
        after,
    )
    .await?;
    let out = fetch_vehicle(&mut tx, id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(out)))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct UpdateVehicle {
    pub kind: Option<String>,
    pub capacity_kg: Option<Decimal>,
    pub status: Option<String>,
    pub home_site_id: Option<Uuid>,
}

#[utoipa::path(patch, path = "/api/v1/ops/vehicles/{id}", tag = "ops", security(("bearer" = [])),
    request_body = UpdateVehicle,
    responses((status = 200, body = VehicleOut), (status = 404, body = Problem)))]
pub async fn update_vehicle(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<UpdateVehicle>,
) -> ApiResult<Json<VehicleOut>> {
    actor.require("fleet:manage")?;
    check_not_negative("capacity_kg", body.capacity_kg)?;
    if let Some(kind) = &body.kind {
        service::check_one_of("kind", kind, &service::VEHICLE_KINDS)?;
    }
    if let Some(status) = &body.status {
        service::check_one_of("status", status, &service::VEHICLE_STATUSES)?;
    }
    let mut tx = state.pool.begin().await?;
    let before = audit::snapshot(&mut tx, "vehicles", id)
        .await?
        .ok_or_else(|| ApiError::not_found("vehicle"))?;
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("update vehicles set ");
    let mut sets = qb.separated(", ");
    let mut changed = false;
    if let Some(v) = &body.kind {
        sets.push("kind = ").push_bind_unseparated(v.clone());
        changed = true;
    }
    if let Some(v) = body.capacity_kg {
        sets.push("capacity_kg = ").push_bind_unseparated(v);
        changed = true;
    }
    if let Some(v) = &body.status {
        sets.push("status = ").push_bind_unseparated(v.clone());
        changed = true;
    }
    if let Some(v) = body.home_site_id {
        sets.push("home_site_id = ").push_bind_unseparated(v);
        changed = true;
    }
    if !changed {
        return Err(ApiError::validation("body", "no fields to update"));
    }
    qb.push(" where id = ").push_bind(id);
    qb.build().execute(&mut *tx).await?;
    let after = audit::snapshot(&mut tx, "vehicles", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "vehicle.update",
        "vehicle",
        Some(id),
        Some(before),
        after,
    )
    .await?;
    let out = fetch_vehicle(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(out))
}
