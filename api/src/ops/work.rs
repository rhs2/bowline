//! Work orders (the ground-staff task list) and warehouse inventory.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
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
use crate::org::service as org;
use crate::outbox;
use crate::scope::Scope;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// Work orders
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct WorkOrderOut {
    pub id: Uuid,
    pub shipment_id: Option<Uuid>,
    pub shipment_reference: Option<String>,
    pub site_id: Option<Uuid>,
    pub site_name: Option<String>,
    pub kind: String,
    pub title: String,
    pub instructions: Option<String>,
    pub assigned_to: Option<Uuid>,
    pub assignee_name: Option<String>,
    pub assigned_by: Option<Uuid>,
    pub status: String,
    pub due_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub const WORK_ORDER_SELECT: &str =
    "select w.id, w.shipment_id, sh.reference as shipment_reference, w.site_id, si.name as site_name,
            w.kind, w.title, w.instructions, w.assigned_to,
            a.first_name || ' ' || a.last_name as assignee_name, w.assigned_by, w.status, w.due_at,
            w.started_at, w.completed_at, w.notes, w.created_at, w.updated_at,
            count(*) over() as total_count
       from work_orders w
       left join shipments sh on sh.id = w.shipment_id
       left join sites si on si.id = w.site_id
       left join employees a on a.id = w.assigned_to
      where ";

async fn fetch_work_order(conn: &mut PgConnection, id: Uuid) -> ApiResult<WorkOrderOut> {
    sqlx::query_as(&format!("{WORK_ORDER_SELECT} w.id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(|| ApiError::not_found("work order"))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct WorkOrderFilter {
    /// `1` for the caller's own list; the default for anyone without
    /// `tasks:manage:subtree`.
    pub mine: Option<String>,
    /// `open`, `in_progress`, `done`, `blocked` or `cancelled`.
    pub status: Option<String>,
    /// `loading`, `unloading`, `pickup`, `delivery`, `inspection` or `inventory`.
    pub kind: Option<String>,
    pub shipment_id: Option<Uuid>,
    pub site_id: Option<Uuid>,
    pub assigned_to: Option<Uuid>,
}

#[utoipa::path(get, path = "/api/v1/ops/work-orders", tag = "ops", security(("bearer" = [])),
    params(PageQuery, WorkOrderFilter), responses((status = 200, body = PageOut<WorkOrderOut>)))]
pub async fn list_work_orders(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<WorkOrderFilter>,
) -> ApiResult<Json<PageOut<WorkOrderOut>>> {
    let mine = service::truthy(filter.mine.as_deref());
    let manages = actor.has("tasks:manage:subtree");
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(WORK_ORDER_SELECT);
    if mine || !manages {
        actor.require("tasks:read:self")?;
        qb.push(" w.assigned_to = ").push_bind(actor.me());
    } else {
        // Supervisors see the work of everyone below them, plus anything they
        // raised themselves that nobody has picked up yet.
        qb.push(" (");
        actor.filter(Scope::Subtree).push(&mut qb, "a");
        qb.push(" or w.assigned_by = ")
            .push_bind(actor.me())
            .push(") ");
    }
    if let Some(status) = &filter.status {
        service::check_one_of("status", status, &service::WORK_ORDER_STATUSES)?;
        qb.push(" and w.status = ").push_bind(status.clone());
    }
    if let Some(kind) = &filter.kind {
        service::check_one_of("kind", kind, &service::WORK_ORDER_KINDS)?;
        qb.push(" and w.kind = ").push_bind(kind.clone());
    }
    if let Some(shipment_id) = filter.shipment_id {
        qb.push(" and w.shipment_id = ").push_bind(shipment_id);
    }
    if let Some(site_id) = filter.site_id {
        qb.push(" and w.site_id = ").push_bind(site_id);
    }
    if let Some(assigned_to) = filter.assigned_to {
        qb.push(" and w.assigned_to = ").push_bind(assigned_to);
    }
    if let Some(q) = page.search() {
        qb.push(" and (w.title ilike ")
            .push_bind(q.clone())
            .push(" or w.instructions ilike ")
            .push_bind(q)
            .push(")");
    }
    let order = page.order_by(&[
        ("due_at", "w.due_at"),
        ("created_at", "w.created_at"),
        ("status", "w.status"),
    ]);
    let order = if page.sort.is_none() {
        "w.due_at asc nulls last, w.created_at desc".to_string()
    } else {
        order
    };
    let paging = page.page();
    qb.push(format!(" order by {order} limit "));
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&state.pool).await?;
    let (items, total) = split_total::<WorkOrderOut>(rows)?;
    Ok(Json(PageOut::new(items, paging, total)))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateWorkOrder {
    pub shipment_id: Option<Uuid>,
    pub site_id: Option<Uuid>,
    /// `loading`, `unloading`, `pickup`, `delivery`, `inspection` or `inventory`.
    pub kind: String,
    #[validate(length(min = 1, max = 200))]
    pub title: String,
    #[validate(length(max = 4000))]
    pub instructions: Option<String>,
    pub assigned_to: Option<Uuid>,
    pub due_at: Option<DateTime<Utc>>,
}

#[utoipa::path(post, path = "/api/v1/ops/work-orders", tag = "ops", security(("bearer" = [])),
    request_body = CreateWorkOrder,
    responses((status = 201, body = WorkOrderOut), (status = 403, body = Problem)))]
pub async fn create_work_order(
    State(state): State<AppState>,
    actor: Actor,
    ValidatedJson(body): ValidatedJson<CreateWorkOrder>,
) -> ApiResult<(StatusCode, Json<WorkOrderOut>)> {
    actor.require_any(&["tasks:manage:subtree", "shipments:assign"])?;
    service::check_one_of("kind", &body.kind, &service::WORK_ORDER_KINDS)?;
    if body.shipment_id.is_none() && body.site_id.is_none() {
        return Err(ApiError::validation(
            "shipment_id",
            "a work order needs a shipment or a site",
        ));
    }
    let mut tx = state.pool.begin().await?;
    if let Some(assignee_id) = body.assigned_to {
        let assignee = org::load_core(&mut tx, assignee_id)
            .await?
            .ok_or_else(|| ApiError::validation("assigned_to", "unknown employee"))?;
        if assignee.status == "terminated" {
            return Err(ApiError::validation(
                "assigned_to",
                "that employee has left the company",
            ));
        }
        // Dispatchers hand work to drivers who report elsewhere; supervisors may
        // only task their own people.
        if !actor.has("shipments:assign") && !actor.principal.is_in_subtree(&assignee.path) {
            return Err(ApiError::forbidden(
                "you can only assign work to people who report up to you",
            ));
        }
    }
    let id: Uuid = sqlx::query_scalar(
        "insert into work_orders (shipment_id, site_id, kind, title, instructions, assigned_to,
                                  assigned_by, due_at)
         values ($1, $2, $3, $4, $5, $6, $7, $8) returning id",
    )
    .bind(body.shipment_id)
    .bind(body.site_id)
    .bind(&body.kind)
    .bind(&body.title)
    .bind(&body.instructions)
    .bind(body.assigned_to)
    .bind(actor.me())
    .bind(body.due_at)
    .fetch_one(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "work_orders", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "work_order.create",
        "work_order",
        Some(id),
        None,
        after,
    )
    .await?;
    if let Some(assignee_id) = body.assigned_to {
        outbox::enqueue_email(
            &mut tx,
            &[assignee_id],
            &format!("New work order: {}", body.title),
            &format!(
                "{} assigned you a {} task: {}",
                actor.principal.full_name(),
                body.kind,
                body.title
            ),
        )
        .await?;
    }
    let out = fetch_work_order(&mut tx, id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(out)))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct UpdateWorkOrderStatus {
    /// `open`, `in_progress`, `done`, `blocked` or `cancelled`.
    pub status: String,
    #[validate(length(max = 4000))]
    pub notes: Option<String>,
}

#[derive(sqlx::FromRow)]
struct WorkOrderAuth {
    status: String,
    assigned_to: Option<Uuid>,
    assigned_by: Option<Uuid>,
    assignee_path: Option<String>,
    title: String,
}

#[utoipa::path(post, path = "/api/v1/ops/work-orders/{id}/status", tag = "ops",
    security(("bearer" = [])), request_body = UpdateWorkOrderStatus,
    responses((status = 200, body = WorkOrderOut), (status = 403, body = Problem),
        (status = 409, body = Problem)))]
pub async fn update_work_order_status(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<UpdateWorkOrderStatus>,
) -> ApiResult<Json<WorkOrderOut>> {
    service::check_one_of("status", &body.status, &service::WORK_ORDER_STATUSES)?;
    let mut tx = state.pool.begin().await?;
    let current: WorkOrderAuth = sqlx::query_as(
        "select w.status, w.assigned_to, w.assigned_by, a.path::text as assignee_path, w.title
           from work_orders w left join employees a on a.id = w.assigned_to
          where w.id = $1
          for update of w",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| ApiError::not_found("work order"))?;
    // Only the worker holding the task, or a manager above that worker, may move it.
    let is_assignee = current.assigned_to == Some(actor.me()) && actor.has("tasks:update:self");
    let manages = actor.has("tasks:manage:subtree")
        && (current
            .assignee_path
            .as_deref()
            .is_some_and(|path| actor.principal.is_in_subtree(path))
            || (current.assigned_to.is_none() && current.assigned_by == Some(actor.me())));
    if !is_assignee && !manages {
        return Err(ApiError::forbidden(
            "only the assignee or their manager may change a work order",
        ));
    }
    if matches!(current.status.as_str(), "done" | "cancelled") {
        return Err(ApiError::transition(&current.status, &body.status));
    }
    let before = audit::snapshot(&mut tx, "work_orders", id).await?;
    sqlx::query(
        "update work_orders
            set status = $2,
                notes = coalesce($3, notes),
                started_at = case when $2 = 'in_progress' and started_at is null then now()
                                  else started_at end,
                completed_at = case when $2 = 'done' then now() else completed_at end,
                updated_at = now()
          where id = $1",
    )
    .bind(id)
    .bind(&body.status)
    .bind(&body.notes)
    .execute(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "work_orders", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "work_order.status",
        "work_order",
        Some(id),
        before,
        after,
    )
    .await?;
    // The person who raised the task hears when it lands or gets stuck.
    if is_assignee && matches!(body.status.as_str(), "done" | "blocked") {
        if let Some(raised_by) = current.assigned_by.filter(|by| *by != actor.me()) {
            outbox::enqueue_email(
                &mut tx,
                &[raised_by],
                &format!("Work order {}: {}", body.status, current.title),
                &format!(
                    "{} marked \"{}\" as {}. {}",
                    actor.principal.full_name(),
                    current.title,
                    body.status,
                    body.notes.as_deref().unwrap_or("No notes were left.")
                ),
            )
            .await?;
        }
    }
    let out = fetch_work_order(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(out))
}

// ---------------------------------------------------------------------------
// Inventory
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct InventoryItemOut {
    pub id: Uuid,
    pub site_id: Uuid,
    pub site_name: String,
    pub shipment_id: Option<Uuid>,
    pub shipment_reference: Option<String>,
    pub description: String,
    pub quantity: i32,
    pub bin: Option<String>,
    pub received_at: DateTime<Utc>,
    pub released_at: Option<DateTime<Utc>>,
}

const INVENTORY_SELECT: &str =
    "select i.id, i.site_id, si.name as site_name, i.shipment_id, sh.reference as shipment_reference,
            i.description, i.quantity, i.bin, i.received_at, i.released_at,
            count(*) over() as total_count
       from inventory_items i
       join sites si on si.id = i.site_id
       left join shipments sh on sh.id = i.shipment_id
      where ";

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct InventoryFilter {
    pub site_id: Option<Uuid>,
    pub shipment_id: Option<Uuid>,
    /// `1` to hide everything that has already been released.
    pub on_hand: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/ops/inventory", tag = "ops", security(("bearer" = [])),
    params(PageQuery, InventoryFilter), responses((status = 200, body = PageOut<InventoryItemOut>)))]
pub async fn list_inventory(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<InventoryFilter>,
) -> ApiResult<Json<PageOut<InventoryItemOut>>> {
    actor.require("shipments:read")?;
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(INVENTORY_SELECT);
    qb.push(" true ");
    if let Some(site_id) = filter.site_id {
        qb.push(" and i.site_id = ").push_bind(site_id);
    }
    if let Some(shipment_id) = filter.shipment_id {
        qb.push(" and i.shipment_id = ").push_bind(shipment_id);
    }
    if service::truthy(filter.on_hand.as_deref()) {
        qb.push(" and i.released_at is null");
    }
    if let Some(q) = page.search() {
        qb.push(" and (i.description ilike ")
            .push_bind(q.clone())
            .push(" or i.bin ilike ")
            .push_bind(q)
            .push(")");
    }
    let paging = page.page();
    qb.push(" order by i.received_at desc limit ");
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&state.pool).await?;
    let (items, total) = split_total::<InventoryItemOut>(rows)?;
    Ok(Json(PageOut::new(items, paging, total)))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateInventoryItem {
    pub site_id: Uuid,
    pub shipment_id: Option<Uuid>,
    #[validate(length(min = 1, max = 500))]
    pub description: String,
    pub quantity: i32,
    #[validate(length(max = 40))]
    pub bin: Option<String>,
    /// Defaults to now.
    pub received_at: Option<DateTime<Utc>>,
}

#[utoipa::path(post, path = "/api/v1/ops/inventory", tag = "ops", security(("bearer" = [])),
    request_body = CreateInventoryItem,
    responses((status = 201, body = InventoryItemOut), (status = 422, body = Problem)))]
pub async fn create_inventory_item(
    State(state): State<AppState>,
    actor: Actor,
    ValidatedJson(body): ValidatedJson<CreateInventoryItem>,
) -> ApiResult<(StatusCode, Json<InventoryItemOut>)> {
    actor.require("shipments:write")?;
    if body.quantity < 0 {
        return Err(ApiError::validation("quantity", "must not be negative"));
    }
    let mut tx = state.pool.begin().await?;
    let id: Uuid = sqlx::query_scalar(
        "insert into inventory_items (site_id, shipment_id, description, quantity, bin, received_at)
         values ($1, $2, $3, $4, $5, coalesce($6, now())) returning id",
    )
    .bind(body.site_id)
    .bind(body.shipment_id)
    .bind(&body.description)
    .bind(body.quantity)
    .bind(&body.bin)
    .bind(body.received_at)
    .fetch_one(&mut *tx)
    .await?;
    let after = audit::snapshot(&mut tx, "inventory_items", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "inventory.create",
        "inventory_item",
        Some(id),
        None,
        after,
    )
    .await?;
    let out: InventoryItemOut = sqlx::query_as(&format!("{INVENTORY_SELECT} i.id = $1"))
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(out)))
}
