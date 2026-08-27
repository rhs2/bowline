//! Shipments: the file itself, its legs, its timeline and its documents.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgConnection, Postgres, QueryBuilder};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;
use validator::Validate;

use crate::audit;
use crate::auth::Actor;
use crate::error::{ApiError, ApiResult, Problem};
use crate::http::extract::ValidatedJson;
use crate::http::pagination::{split_total, PageOut, PageQuery};
use crate::ops::service::{self, RiskInputs, Transition};
use crate::ops::work::{WorkOrderOut, WORK_ORDER_SELECT};
use crate::org::service as org;
use crate::outbox;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Shipments
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct ShipmentSummary {
    pub id: Uuid,
    pub reference: String,
    pub customer_id: Uuid,
    pub customer_name: String,
    pub customer_code: String,
    pub mode: String,
    pub incoterm: Option<String>,
    pub origin: Value,
    pub destination: Value,
    pub cargo_description: String,
    pub pieces: i32,
    pub weight_kg: Decimal,
    pub volume_cbm: Option<Decimal>,
    pub hazardous: bool,
    pub declared_value: Decimal,
    pub currency: String,
    pub status: String,
    pub previous_status: Option<String>,
    pub etd: Option<NaiveDate>,
    pub eta: Option<NaiveDate>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub delay_risk: Option<Decimal>,
    pub owner_id: Option<Uuid>,
    pub owner_name: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

const SHIPMENT_SELECT: &str =
    "select s.id, s.reference, s.customer_id, cu.name as customer_name, cu.code::text as customer_code,
            s.mode, s.incoterm, s.origin, s.destination, s.cargo_description, s.pieces, s.weight_kg,
            s.volume_cbm, s.hazardous, s.declared_value, s.currency, s.status, s.previous_status,
            s.etd, s.eta, s.delivered_at, s.delay_risk, s.owner_id,
            o.first_name || ' ' || o.last_name as owner_name, s.created_by, s.created_at, s.updated_at,
            count(*) over() as total_count
       from shipments s
       join customers cu on cu.id = s.customer_id
       left join employees o on o.id = s.owner_id
      where ";

async fn fetch_shipment(conn: &mut PgConnection, id: Uuid) -> ApiResult<ShipmentSummary> {
    sqlx::query_as(&format!("{SHIPMENT_SELECT} s.id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(|| ApiError::not_found("shipment"))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ShipmentFilter {
    /// One of the nine shipment states.
    pub status: Option<String>,
    pub customer_id: Option<Uuid>,
    /// `sea`, `air`, `road` or `rail`.
    pub mode: Option<String>,
    pub owner_id: Option<Uuid>,
    /// `1` to see only the shipments still moving.
    pub in_flight: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/ops/shipments", tag = "ops", security(("bearer" = [])),
    params(PageQuery, ShipmentFilter), responses((status = 200, body = PageOut<ShipmentSummary>)))]
pub async fn list_shipments(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<ShipmentFilter>,
) -> ApiResult<Json<PageOut<ShipmentSummary>>> {
    actor.require("shipments:read")?;
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(SHIPMENT_SELECT);
    qb.push(" true ");
    if let Some(status) = &filter.status {
        service::check_one_of("status", status, &service::SHIPMENT_STATUSES)?;
        qb.push(" and s.status = ").push_bind(status.clone());
    }
    if let Some(customer) = filter.customer_id {
        qb.push(" and s.customer_id = ").push_bind(customer);
    }
    if let Some(mode) = &filter.mode {
        service::check_one_of("mode", mode, &service::MODES)?;
        qb.push(" and s.mode = ").push_bind(mode.clone());
    }
    if let Some(owner) = filter.owner_id {
        qb.push(" and s.owner_id = ").push_bind(owner);
    }
    if service::truthy(filter.in_flight.as_deref()) {
        qb.push(" and s.status not in ('draft','delivered','cancelled')");
    }
    if let Some(q) = page.search() {
        qb.push(" and (s.reference ilike ")
            .push_bind(q.clone())
            .push(" or s.cargo_description ilike ")
            .push_bind(q.clone())
            .push(" or cu.name ilike ")
            .push_bind(q)
            .push(")");
    }
    let order = page.order_by(&[
        ("created_at", "s.created_at"),
        ("reference", "s.reference"),
        ("eta", "s.eta"),
        ("etd", "s.etd"),
        ("status", "s.status"),
        ("delay_risk", "s.delay_risk"),
    ]);
    let paging = page.page();
    qb.push(format!(" order by {order} limit "));
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&state.pool).await?;
    let (items, total) = split_total::<ShipmentSummary>(rows)?;
    Ok(Json(PageOut::new(items, paging, total)))
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct LegOut {
    pub id: Uuid,
    pub shipment_id: Uuid,
    pub seq: i16,
    pub mode: String,
    pub carrier_id: Option<Uuid>,
    pub carrier_name: Option<String>,
    pub vehicle_id: Option<Uuid>,
    pub vehicle_plate: Option<String>,
    pub driver_id: Option<Uuid>,
    pub driver_name: Option<String>,
    /// Written as `from` on the way in, read back under the column name.
    pub from_location: Value,
    pub to_location: Value,
    pub planned_departure: Option<DateTime<Utc>>,
    pub planned_arrival: Option<DateTime<Utc>>,
    pub actual_departure: Option<DateTime<Utc>>,
    pub actual_arrival: Option<DateTime<Utc>>,
    pub status: String,
}

const LEG_SELECT: &str =
    "select l.id, l.shipment_id, l.seq, l.mode, l.carrier_id, c.name as carrier_name, l.vehicle_id,
            v.plate::text as vehicle_plate, l.driver_id,
            d.first_name || ' ' || d.last_name as driver_name, l.from_location, l.to_location,
            l.planned_departure, l.planned_arrival, l.actual_departure, l.actual_arrival, l.status
       from shipment_legs l
       left join carriers c on c.id = l.carrier_id
       left join vehicles v on v.id = l.vehicle_id
       left join employees d on d.id = l.driver_id
      where ";

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct EventOut {
    pub id: Uuid,
    pub shipment_id: Uuid,
    pub leg_id: Option<Uuid>,
    pub event_type: String,
    pub occurred_at: DateTime<Utc>,
    pub location: Option<String>,
    pub note: Option<String>,
    pub recorded_by: Option<Uuid>,
    pub recorded_by_name: Option<String>,
}

const EVENT_SELECT: &str =
    "select e.id, e.shipment_id, e.leg_id, e.event_type, e.occurred_at, e.location, e.note,
            e.recorded_by, r.first_name || ' ' || r.last_name as recorded_by_name
       from shipment_events e left join employees r on r.id = e.recorded_by
      where ";

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct ShipmentDocumentOut {
    pub id: Uuid,
    pub shipment_id: Uuid,
    pub kind: String,
    pub title: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub uploaded_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

const SHIPMENT_DOCUMENT_SELECT: &str =
    "select d.id, d.shipment_id, d.kind, d.title, d.mime_type, d.size_bytes, d.uploaded_by,
            d.created_at
       from shipment_documents d
      where ";

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct InvoiceRef {
    pub id: Uuid,
    pub invoice_no: String,
    pub status: String,
    pub currency: String,
    pub total: Decimal,
    pub amount_paid: Decimal,
    pub issue_date: Option<NaiveDate>,
    pub due_date: Option<NaiveDate>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ShipmentDetail {
    #[serde(flatten)]
    pub shipment: ShipmentSummary,
    pub legs: Vec<LegOut>,
    /// Newest first.
    pub events: Vec<EventOut>,
    pub documents: Vec<ShipmentDocumentOut>,
    pub work_orders: Vec<WorkOrderOut>,
    pub invoice: Option<InvoiceRef>,
    /// The states `POST /transition` will accept from here, in pipeline order.
    pub allowed_transitions: Vec<String>,
}

async fn shipment_detail(conn: &mut PgConnection, id: Uuid) -> ApiResult<ShipmentDetail> {
    let shipment = fetch_shipment(&mut *conn, id).await?;
    let legs: Vec<LegOut> =
        sqlx::query_as(&format!("{LEG_SELECT} l.shipment_id = $1 order by l.seq"))
            .bind(id)
            .fetch_all(&mut *conn)
            .await?;
    let events: Vec<EventOut> = sqlx::query_as(&format!(
        "{EVENT_SELECT} e.shipment_id = $1 order by e.occurred_at desc, e.created_at desc"
    ))
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    let documents: Vec<ShipmentDocumentOut> = sqlx::query_as(&format!(
        "{SHIPMENT_DOCUMENT_SELECT} d.shipment_id = $1 order by d.created_at desc"
    ))
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    let work_orders: Vec<WorkOrderOut> = sqlx::query_as(&format!(
        "{WORK_ORDER_SELECT} w.shipment_id = $1 order by w.due_at nulls last, w.created_at"
    ))
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    let invoice: Option<InvoiceRef> = sqlx::query_as(
        "select id, invoice_no, status, currency, total, amount_paid, issue_date, due_date
           from invoices where shipment_id = $1 order by created_at desc limit 1",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    let allowed_transitions =
        service::allowed_transitions(&shipment.status, shipment.previous_status.as_deref());
    Ok(ShipmentDetail {
        shipment,
        legs,
        events,
        documents,
        work_orders,
        invoice,
        allowed_transitions,
    })
}

#[utoipa::path(get, path = "/api/v1/ops/shipments/{id}", tag = "ops", security(("bearer" = [])),
    responses((status = 200, body = ShipmentDetail), (status = 404, body = Problem)))]
pub async fn get_shipment(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ShipmentDetail>> {
    actor.require("shipments:read")?;
    let mut conn = state.pool.acquire().await?;
    Ok(Json(shipment_detail(&mut conn, id).await?))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateShipment {
    pub customer_id: Uuid,
    /// `sea`, `air`, `road` or `rail`.
    pub mode: String,
    /// `EXW`, `FCA`, `FOB`, `CIF`, `DAP` or `DDP`.
    pub incoterm: Option<String>,
    /// `{city, country, port}`.
    pub origin: Value,
    pub destination: Value,
    #[validate(length(min = 1, max = 2000))]
    pub cargo_description: String,
    pub pieces: Option<i32>,
    pub weight_kg: Decimal,
    pub volume_cbm: Option<Decimal>,
    pub hazardous: Option<bool>,
    pub declared_value: Option<Decimal>,
    pub currency: Option<String>,
    pub etd: Option<NaiveDate>,
    pub eta: Option<NaiveDate>,
    /// The coordinator who owns the file; defaults to the caller.
    pub owner_id: Option<Uuid>,
}

/// The cargo fields create and patch have in common, checked in one place. Every
/// field is optional so a patch can hand over only what it touches.
#[derive(Default)]
struct CargoFields<'a> {
    mode: Option<&'a str>,
    incoterm: Option<&'a str>,
    origin: Option<&'a Value>,
    destination: Option<&'a Value>,
    pieces: Option<i32>,
    weight_kg: Option<Decimal>,
    volume_cbm: Option<Decimal>,
    declared_value: Option<Decimal>,
    etd: Option<NaiveDate>,
    eta: Option<NaiveDate>,
}

impl CargoFields<'_> {
    fn check(&self) -> ApiResult<()> {
        if let Some(mode) = self.mode {
            service::check_one_of("mode", mode, &service::MODES)?;
        }
        if let Some(incoterm) = self.incoterm {
            service::check_one_of("incoterm", incoterm, &service::INCOTERMS)?;
        }
        if let Some(origin) = self.origin {
            service::check_json_object("origin", origin)?;
        }
        if let Some(destination) = self.destination {
            service::check_json_object("destination", destination)?;
        }
        if self.pieces.is_some_and(|p| p < 1) {
            return Err(ApiError::validation("pieces", "must be at least 1"));
        }
        if self.weight_kg.is_some_and(|w| w < Decimal::ZERO) {
            return Err(ApiError::validation("weight_kg", "must not be negative"));
        }
        if self.volume_cbm.is_some_and(|v| v < Decimal::ZERO) {
            return Err(ApiError::validation("volume_cbm", "must not be negative"));
        }
        if self.declared_value.is_some_and(|v| v < Decimal::ZERO) {
            return Err(ApiError::validation(
                "declared_value",
                "must not be negative",
            ));
        }
        if let (Some(etd), Some(eta)) = (self.etd, self.eta) {
            if eta < etd {
                return Err(ApiError::validation("eta", "must not be before etd"));
            }
        }
        Ok(())
    }
}

#[utoipa::path(post, path = "/api/v1/ops/shipments", tag = "ops", security(("bearer" = [])),
    request_body = CreateShipment,
    responses((status = 201, body = ShipmentDetail), (status = 422, body = Problem)))]
pub async fn create_shipment(
    State(state): State<AppState>,
    actor: Actor,
    ValidatedJson(body): ValidatedJson<CreateShipment>,
) -> ApiResult<(StatusCode, Json<ShipmentDetail>)> {
    actor.require("shipments:write")?;
    CargoFields {
        mode: Some(&body.mode),
        incoterm: body.incoterm.as_deref(),
        origin: Some(&body.origin),
        destination: Some(&body.destination),
        pieces: body.pieces,
        weight_kg: Some(body.weight_kg),
        volume_cbm: body.volume_cbm,
        declared_value: body.declared_value,
        etd: body.etd,
        eta: body.eta,
    }
    .check()?;
    let currency = service::currency_or_default(body.currency.as_deref())?;
    let pieces = body.pieces.unwrap_or(1);
    let hazardous = body.hazardous.unwrap_or(false);
    // Scored before the write so the risk lands with the row. Analytics failing
    // only costs the shipment its score.
    let delay_risk = service::delay_risk(
        &state.analytics,
        &RiskInputs {
            mode: body.mode.clone(),
            weight_kg: body.weight_kg,
            pieces,
            hazardous,
            etd: body.etd,
            eta: body.eta,
            carrier_on_time_rate: None,
        },
    )
    .await;
    let owner_id = body.owner_id.unwrap_or_else(|| actor.me());
    let mut tx = state.pool.begin().await?;
    let reference = org::next_shipment_ref(&mut tx).await?;
    let id: Uuid = sqlx::query_scalar(
        "insert into shipments (reference, customer_id, mode, incoterm, origin, destination,
                                cargo_description, pieces, weight_kg, volume_cbm, hazardous,
                                declared_value, currency, etd, eta, delay_risk, owner_id, created_by)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
         returning id",
    )
    .bind(&reference)
    .bind(body.customer_id)
    .bind(&body.mode)
    .bind(&body.incoterm)
    .bind(&body.origin)
    .bind(&body.destination)
    .bind(&body.cargo_description)
    .bind(pieces)
    .bind(body.weight_kg)
    .bind(body.volume_cbm)
    .bind(hazardous)
    .bind(body.declared_value.unwrap_or(Decimal::ZERO))
    .bind(&currency)
    .bind(body.etd)
    .bind(body.eta)
    .bind(delay_risk)
    .bind(owner_id)
    .bind(actor.me())
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "insert into shipment_events (shipment_id, event_type, note, recorded_by)
         values ($1, 'created', $2, $3)",
    )
    .bind(id)
    .bind(format!("Shipment {reference} created"))
    .bind(actor.me())
    .execute(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "shipments", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "shipment.create",
        "shipment",
        Some(id),
        None,
        after,
    )
    .await?;
    let detail = shipment_detail(&mut tx, id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(detail)))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct UpdateShipment {
    pub mode: Option<String>,
    pub incoterm: Option<String>,
    pub origin: Option<Value>,
    pub destination: Option<Value>,
    #[validate(length(min = 1, max = 2000))]
    pub cargo_description: Option<String>,
    pub pieces: Option<i32>,
    pub weight_kg: Option<Decimal>,
    pub volume_cbm: Option<Decimal>,
    pub hazardous: Option<bool>,
    pub declared_value: Option<Decimal>,
    pub currency: Option<String>,
    pub etd: Option<NaiveDate>,
    pub eta: Option<NaiveDate>,
    pub owner_id: Option<Uuid>,
}

#[utoipa::path(patch, path = "/api/v1/ops/shipments/{id}", tag = "ops", security(("bearer" = [])),
    request_body = UpdateShipment,
    responses((status = 200, body = ShipmentDetail), (status = 404, body = Problem)))]
pub async fn update_shipment(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<UpdateShipment>,
) -> ApiResult<Json<ShipmentDetail>> {
    actor.require("shipments:write")?;
    let mut tx = state.pool.begin().await?;
    let current = service::lock_shipment(&mut tx, id).await?;
    if service::is_terminal(&current.status) {
        return Err(ApiError::conflict(format!(
            "a {} shipment can no longer be edited",
            current.status
        )));
    }
    CargoFields {
        mode: body.mode.as_deref(),
        incoterm: body.incoterm.as_deref(),
        origin: body.origin.as_ref(),
        destination: body.destination.as_ref(),
        pieces: body.pieces,
        weight_kg: body.weight_kg,
        volume_cbm: body.volume_cbm,
        declared_value: body.declared_value,
        etd: body.etd,
        eta: body.eta,
    }
    .check()?;
    let before = audit::snapshot(&mut tx, "shipments", id).await?;
    let mut qb: QueryBuilder<Postgres> =
        QueryBuilder::new("update shipments set updated_at = now()");
    if let Some(v) = &body.mode {
        qb.push(", mode = ").push_bind(v.clone());
    }
    if let Some(v) = &body.incoterm {
        qb.push(", incoterm = ").push_bind(v.clone());
    }
    if let Some(v) = &body.origin {
        qb.push(", origin = ").push_bind(v.clone());
    }
    if let Some(v) = &body.destination {
        qb.push(", destination = ").push_bind(v.clone());
    }
    if let Some(v) = &body.cargo_description {
        qb.push(", cargo_description = ").push_bind(v.clone());
    }
    if let Some(v) = body.pieces {
        qb.push(", pieces = ").push_bind(v);
    }
    if let Some(v) = body.weight_kg {
        qb.push(", weight_kg = ").push_bind(v);
    }
    if let Some(v) = body.volume_cbm {
        qb.push(", volume_cbm = ").push_bind(v);
    }
    if let Some(v) = body.hazardous {
        qb.push(", hazardous = ").push_bind(v);
    }
    if let Some(v) = body.declared_value {
        qb.push(", declared_value = ").push_bind(v);
    }
    if let Some(v) = &body.currency {
        qb.push(", currency = ")
            .push_bind(service::currency_or_default(Some(v))?);
    }
    if let Some(v) = body.etd {
        qb.push(", etd = ").push_bind(v);
    }
    if let Some(v) = body.eta {
        qb.push(", eta = ").push_bind(v);
    }
    if let Some(v) = body.owner_id {
        qb.push(", owner_id = ").push_bind(v);
    }
    qb.push(" where id = ").push_bind(id);
    qb.build().execute(&mut *tx).await?;
    let after = audit::snapshot(&mut tx, "shipments", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "shipment.update",
        "shipment",
        Some(id),
        before,
        after,
    )
    .await?;
    let detail = shipment_detail(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(detail))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct TransitionShipment {
    /// The state to move to.
    pub to: String,
    #[validate(length(max = 2000))]
    pub note: Option<String>,
    #[validate(length(max = 200))]
    pub location: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/ops/shipments/{id}/transition", tag = "ops", security(("bearer" = [])),
    request_body = TransitionShipment,
    responses((status = 200, body = ShipmentDetail), (status = 409, body = Problem)))]
pub async fn transition_shipment(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<TransitionShipment>,
) -> ApiResult<Json<ShipmentDetail>> {
    actor.require("shipments:write")?;
    service::check_one_of("to", &body.to, &service::SHIPMENT_STATUSES)?;
    // A first, unlocked read decides whether this transition is worth an analytics
    // call; the locked read inside the transaction is the one that decides.
    let mut conn = state.pool.acquire().await?;
    let preview = service::load_shipment(&mut conn, id).await?;
    service::plan_transition(
        &preview.status,
        &body.to,
        preview.previous_status.as_deref(),
    )?;
    let delay_risk = if body.to == "booked" {
        match service::load_risk_inputs(&mut conn, id).await? {
            Some(inputs) => service::delay_risk(&state.analytics, &inputs).await,
            None => None,
        }
    } else {
        None
    };
    drop(conn);

    let mut tx = state.pool.begin().await?;
    let current = service::lock_shipment(&mut tx, id).await?;
    let plan = service::plan_transition(
        &current.status,
        &body.to,
        current.previous_status.as_deref(),
    )?;
    let previous_status = match plan {
        Transition::Exception => Some(current.status.clone()),
        _ => None,
    };
    let before = audit::snapshot(&mut tx, "shipments", id).await?;
    sqlx::query(
        "update shipments
            set status = $2,
                previous_status = $3,
                delivered_at = case when $2 = 'delivered' then now() else delivered_at end,
                delay_risk = coalesce($4, delay_risk),
                updated_at = now()
          where id = $1",
    )
    .bind(id)
    .bind(&body.to)
    .bind(&previous_status)
    .bind(delay_risk)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "insert into shipment_events (shipment_id, event_type, location, note, recorded_by)
         values ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(service::event_for(&body.to, plan))
    .bind(&body.location)
    .bind(&body.note)
    .bind(actor.me())
    .execute(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "shipments", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "shipment.transition",
        "shipment",
        Some(id),
        before,
        after,
    )
    .await?;
    // The owner needs to know when their file stops moving.
    if plan == Transition::Exception {
        if let Some(owner) = current.owner_id.filter(|owner| *owner != actor.me()) {
            outbox::enqueue_email(
                &mut tx,
                &[owner],
                &format!("Shipment {} hit an exception", current.reference),
                &format!(
                    "{} moved {} from {} to exception. {}",
                    actor.principal.full_name(),
                    current.reference,
                    current.status,
                    body.note.as_deref().unwrap_or("No note was left.")
                ),
            )
            .await?;
        }
    }
    let detail = shipment_detail(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(detail))
}

// ---------------------------------------------------------------------------
// Legs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateLeg {
    /// Position in the journey, starting at 1.
    pub seq: i16,
    /// `sea`, `air`, `road` or `rail`.
    pub mode: String,
    pub carrier_id: Option<Uuid>,
    pub vehicle_id: Option<Uuid>,
    pub driver_id: Option<Uuid>,
    #[serde(rename = "from")]
    pub from_location: Value,
    #[serde(rename = "to")]
    pub to_location: Value,
    pub planned_departure: Option<DateTime<Utc>>,
    pub planned_arrival: Option<DateTime<Utc>>,
}

#[utoipa::path(post, path = "/api/v1/ops/shipments/{id}/legs", tag = "ops", security(("bearer" = [])),
    request_body = CreateLeg, responses((status = 201, body = LegOut), (status = 409, body = Problem)))]
pub async fn create_leg(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<CreateLeg>,
) -> ApiResult<(StatusCode, Json<LegOut>)> {
    actor.require("shipments:assign")?;
    service::check_one_of("mode", &body.mode, &service::MODES)?;
    service::check_json_object("from", &body.from_location)?;
    service::check_json_object("to", &body.to_location)?;
    if body.seq < 1 {
        return Err(ApiError::validation("seq", "must be at least 1"));
    }
    if let (Some(departure), Some(arrival)) = (body.planned_departure, body.planned_arrival) {
        if arrival < departure {
            return Err(ApiError::validation(
                "planned_arrival",
                "must not be before planned_departure",
            ));
        }
    }
    let mut tx = state.pool.begin().await?;
    let shipment = service::load_shipment(&mut tx, id).await?;
    if service::is_terminal(&shipment.status) {
        return Err(ApiError::conflict(format!(
            "a {} shipment can no longer be routed",
            shipment.status
        )));
    }
    let leg_id: Uuid = sqlx::query_scalar(
        "insert into shipment_legs (shipment_id, seq, mode, carrier_id, vehicle_id, driver_id,
                                    from_location, to_location, planned_departure, planned_arrival)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) returning id",
    )
    .bind(id)
    .bind(body.seq)
    .bind(&body.mode)
    .bind(body.carrier_id)
    .bind(body.vehicle_id)
    .bind(body.driver_id)
    .bind(&body.from_location)
    .bind(&body.to_location)
    .bind(body.planned_departure)
    .bind(body.planned_arrival)
    .fetch_one(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "shipment_legs", leg_id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "shipment.leg_create",
        "shipment_leg",
        Some(leg_id),
        None,
        after,
    )
    .await?;
    if let Some(driver) = body.driver_id {
        outbox::enqueue_email(
            &mut tx,
            &[driver],
            &format!("You are driving leg {} of {}", body.seq, shipment.reference),
            &format!(
                "Leg {} of shipment {} has been assigned to you.",
                body.seq, shipment.reference
            ),
        )
        .await?;
    }
    let out: LegOut = sqlx::query_as(&format!("{LEG_SELECT} l.id = $1"))
        .bind(leg_id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    // A new carrier changes the risk picture.
    service::rescore(&state, id).await;
    Ok((StatusCode::CREATED, Json(out)))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct UpdateLeg {
    pub mode: Option<String>,
    pub carrier_id: Option<Uuid>,
    pub vehicle_id: Option<Uuid>,
    pub driver_id: Option<Uuid>,
    #[serde(rename = "from")]
    pub from_location: Option<Value>,
    #[serde(rename = "to")]
    pub to_location: Option<Value>,
    pub planned_departure: Option<DateTime<Utc>>,
    pub planned_arrival: Option<DateTime<Utc>>,
    pub actual_departure: Option<DateTime<Utc>>,
    pub actual_arrival: Option<DateTime<Utc>>,
    /// `planned`, `in_progress`, `completed` or `cancelled`.
    pub status: Option<String>,
}

#[utoipa::path(patch, path = "/api/v1/ops/shipments/{id}/legs/{leg_id}", tag = "ops",
    security(("bearer" = [])), request_body = UpdateLeg,
    responses((status = 200, body = LegOut), (status = 404, body = Problem)))]
pub async fn update_leg(
    State(state): State<AppState>,
    actor: Actor,
    Path((id, leg_id)): Path<(Uuid, Uuid)>,
    ValidatedJson(body): ValidatedJson<UpdateLeg>,
) -> ApiResult<Json<LegOut>> {
    actor.require("shipments:assign")?;
    if let Some(mode) = &body.mode {
        service::check_one_of("mode", mode, &service::MODES)?;
    }
    if let Some(status) = &body.status {
        service::check_one_of("status", status, &service::LEG_STATUSES)?;
    }
    if let Some(from) = &body.from_location {
        service::check_json_object("from", from)?;
    }
    if let Some(to) = &body.to_location {
        service::check_json_object("to", to)?;
    }
    let mut tx = state.pool.begin().await?;
    let belongs: bool = sqlx::query_scalar(
        "select exists (select 1 from shipment_legs where id = $1 and shipment_id = $2)",
    )
    .bind(leg_id)
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if !belongs {
        return Err(ApiError::not_found("leg"));
    }
    let before = audit::snapshot(&mut tx, "shipment_legs", leg_id).await?;
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("update shipment_legs set ");
    let mut sets = qb.separated(", ");
    let mut changed = false;
    if let Some(v) = &body.mode {
        sets.push("mode = ").push_bind_unseparated(v.clone());
        changed = true;
    }
    if let Some(v) = body.carrier_id {
        sets.push("carrier_id = ").push_bind_unseparated(v);
        changed = true;
    }
    if let Some(v) = body.vehicle_id {
        sets.push("vehicle_id = ").push_bind_unseparated(v);
        changed = true;
    }
    if let Some(v) = body.driver_id {
        sets.push("driver_id = ").push_bind_unseparated(v);
        changed = true;
    }
    if let Some(v) = &body.from_location {
        sets.push("from_location = ")
            .push_bind_unseparated(v.clone());
        changed = true;
    }
    if let Some(v) = &body.to_location {
        sets.push("to_location = ").push_bind_unseparated(v.clone());
        changed = true;
    }
    if let Some(v) = body.planned_departure {
        sets.push("planned_departure = ").push_bind_unseparated(v);
        changed = true;
    }
    if let Some(v) = body.planned_arrival {
        sets.push("planned_arrival = ").push_bind_unseparated(v);
        changed = true;
    }
    if let Some(v) = body.actual_departure {
        sets.push("actual_departure = ").push_bind_unseparated(v);
        changed = true;
    }
    if let Some(v) = body.actual_arrival {
        sets.push("actual_arrival = ").push_bind_unseparated(v);
        changed = true;
    }
    if let Some(v) = &body.status {
        sets.push("status = ").push_bind_unseparated(v.clone());
        changed = true;
    }
    if !changed {
        return Err(ApiError::validation("body", "no fields to update"));
    }
    qb.push(" where id = ").push_bind(leg_id);
    qb.build().execute(&mut *tx).await?;
    let after = audit::snapshot(&mut tx, "shipment_legs", leg_id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "shipment.leg_update",
        "shipment_leg",
        Some(leg_id),
        before,
        after,
    )
    .await?;
    let out: LegOut = sqlx::query_as(&format!("{LEG_SELECT} l.id = $1"))
        .bind(leg_id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    service::rescore(&state, id).await;
    Ok(Json(out))
}

// ---------------------------------------------------------------------------
// Timeline
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateEvent {
    /// One of the tracking event types, or `note`.
    pub event_type: String,
    pub leg_id: Option<Uuid>,
    #[validate(length(max = 200))]
    pub location: Option<String>,
    #[validate(length(max = 2000))]
    pub note: Option<String>,
    /// Defaults to now; useful when a driver reports late.
    pub occurred_at: Option<DateTime<Utc>>,
}

#[utoipa::path(post, path = "/api/v1/ops/shipments/{id}/events", tag = "ops", security(("bearer" = [])),
    request_body = CreateEvent, responses((status = 201, body = EventOut), (status = 403, body = Problem)))]
pub async fn create_event(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<CreateEvent>,
) -> ApiResult<(StatusCode, Json<EventOut>)> {
    service::check_one_of("event_type", &body.event_type, &service::MANUAL_EVENT_TYPES)?;
    let mut tx = state.pool.begin().await?;
    service::load_shipment(&mut tx, id).await?;
    // Coordinators write the timeline; so do the drivers actually moving the cargo.
    if !actor.has("shipments:write")
        && !service::is_assigned_driver(&mut tx, id, actor.me()).await?
    {
        return Err(ApiError::forbidden(
            "requires shipments:write or a leg assigned to you",
        ));
    }
    if let Some(leg_id) = body.leg_id {
        let belongs: bool = sqlx::query_scalar(
            "select exists (select 1 from shipment_legs where id = $1 and shipment_id = $2)",
        )
        .bind(leg_id)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if !belongs {
            return Err(ApiError::validation("leg_id", "not a leg of this shipment"));
        }
    }
    let event_id: Uuid = sqlx::query_scalar(
        "insert into shipment_events (shipment_id, leg_id, event_type, occurred_at, location, note,
                                      recorded_by)
         values ($1, $2, $3, coalesce($4, now()), $5, $6, $7) returning id",
    )
    .bind(id)
    .bind(body.leg_id)
    .bind(&body.event_type)
    .bind(body.occurred_at)
    .bind(&body.location)
    .bind(&body.note)
    .bind(actor.me())
    .fetch_one(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "shipment_events", event_id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "shipment.event",
        "shipment_event",
        Some(event_id),
        None,
        after,
    )
    .await?;
    let out: EventOut = sqlx::query_as(&format!("{EVENT_SELECT} e.id = $1"))
        .bind(event_id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(out)))
}

// ---------------------------------------------------------------------------
// Documents
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct PresignShipmentDocument {
    /// `bill_of_lading`, `air_waybill`, `commercial_invoice`, `packing_list`,
    /// `customs`, `proof_of_delivery` or `other`.
    #[validate(length(min = 1, max = 30))]
    pub kind: String,
    #[validate(length(min = 1, max = 200))]
    pub title: String,
    #[validate(length(min = 3, max = 120))]
    pub mime_type: String,
    pub size_bytes: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ShipmentUploadUrl {
    pub upload_url: String,
    pub s3_key: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ShipmentDownloadUrl {
    pub url: String,
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

#[utoipa::path(post, path = "/api/v1/ops/shipments/{id}/documents/presign", tag = "ops",
    security(("bearer" = [])), request_body = PresignShipmentDocument,
    responses((status = 200, body = ShipmentUploadUrl), (status = 404, body = Problem)))]
pub async fn presign_shipment_document(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<PresignShipmentDocument>,
) -> ApiResult<Json<ShipmentUploadUrl>> {
    actor.require("shipments:write")?;
    check_upload(&body.kind, &body.mime_type, body.size_bytes)?;
    let mut conn = state.pool.acquire().await?;
    service::load_shipment(&mut conn, id).await?;
    let s3_key = service::document_key(id, &body.kind, &body.title);
    let upload_url = state
        .s3
        .presign_put(&state.s3.bucket_documents, &s3_key, &body.mime_type)
        .await?;
    Ok(Json(ShipmentUploadUrl { upload_url, s3_key }))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct ConfirmShipmentDocument {
    #[validate(length(min = 1, max = 30))]
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

#[utoipa::path(post, path = "/api/v1/ops/shipments/{id}/documents", tag = "ops",
    security(("bearer" = [])), request_body = ConfirmShipmentDocument,
    responses((status = 201, body = ShipmentDocumentOut), (status = 409, body = Problem)))]
pub async fn confirm_shipment_document(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<ConfirmShipmentDocument>,
) -> ApiResult<(StatusCode, Json<ShipmentDocumentOut>)> {
    actor.require("shipments:write")?;
    check_upload(&body.kind, &body.mime_type, body.size_bytes)?;
    if !body.s3_key.starts_with(&service::document_prefix(id)) {
        return Err(ApiError::validation(
            "s3_key",
            "does not belong to that shipment",
        ));
    }
    let mut tx = state.pool.begin().await?;
    service::load_shipment(&mut tx, id).await?;
    let doc_id: Uuid = sqlx::query_scalar(
        "insert into shipment_documents (shipment_id, kind, title, s3_key, mime_type, size_bytes,
                                         uploaded_by)
         values ($1, $2, $3, $4, $5, $6, $7) returning id",
    )
    .bind(id)
    .bind(&body.kind)
    .bind(&body.title)
    .bind(&body.s3_key)
    .bind(&body.mime_type)
    .bind(body.size_bytes)
    .bind(actor.me())
    .fetch_one(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "shipment_documents", doc_id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "shipment.document",
        "shipment_document",
        Some(doc_id),
        None,
        after,
    )
    .await?;
    let out: ShipmentDocumentOut = sqlx::query_as(&format!("{SHIPMENT_DOCUMENT_SELECT} d.id = $1"))
        .bind(doc_id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(out)))
}

#[utoipa::path(get, path = "/api/v1/ops/shipments/{id}/documents/{doc_id}/download", tag = "ops",
    security(("bearer" = [])),
    responses((status = 200, body = ShipmentDownloadUrl), (status = 404, body = Problem)))]
pub async fn download_shipment_document(
    State(state): State<AppState>,
    actor: Actor,
    Path((id, doc_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<ShipmentDownloadUrl>> {
    actor.require("shipments:read")?;
    let mut conn = state.pool.acquire().await?;
    let s3_key: String = sqlx::query_scalar(
        "select s3_key from shipment_documents where id = $1 and shipment_id = $2",
    )
    .bind(doc_id)
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(|| ApiError::not_found("document"))?;
    let url = state
        .s3
        .presign_get(&state.s3.bucket_documents, &s3_key)
        .await?;
    Ok(Json(ShipmentDownloadUrl { url }))
}
