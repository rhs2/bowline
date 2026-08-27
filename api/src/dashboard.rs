//! One role-aware summary endpoint. Every block is optional and appears only when the
//! caller holds the permission behind it, so a driver sees their tasks and their inbox
//! while a finance director also sees receivables and headcount.

use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::PgConnection;
use utoipa::{OpenApi, ToSchema};
use uuid::Uuid;

use crate::auth::Actor;
use crate::error::ApiResult;
use crate::state::AppState;

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct WorkOrderBrief {
    pub id: Uuid,
    pub kind: String,
    pub title: String,
    pub status: String,
    pub due_at: Option<DateTime<Utc>>,
    pub shipment_reference: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MyWork {
    pub open: i64,
    pub overdue: i64,
    pub next: Vec<WorkOrderBrief>,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct MyLeave {
    pub pending: i64,
    pub upcoming_approved: i64,
    pub next_start: Option<NaiveDate>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct WaitingOnMe {
    pub leave_requests: i64,
    pub expense_claims: i64,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct MyMessages {
    pub unread: i64,
    pub threads: i64,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct MyTickets {
    pub open: i64,
    pub assigned_to_me: i64,
    pub awaiting_my_close: i64,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct StatusCount {
    pub status: String,
    pub count: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ShipmentsInFlight {
    pub total: i64,
    pub by_status: Vec<StatusCount>,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct Receivables {
    pub outstanding: Decimal,
    pub overdue: Decimal,
    pub open_invoices: i64,
    pub overdue_invoices: i64,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct People {
    pub headcount: i64,
    pub on_leave_today: i64,
    pub joined_this_month: i64,
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct ServiceDesk {
    pub open: i64,
    pub unassigned: i64,
    pub breaching_sla: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Dashboard {
    pub employee_id: Uuid,
    pub name: String,
    pub title: String,
    pub roles: Vec<String>,
    pub generated_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub my_work: Option<MyWork>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub my_leave: Option<MyLeave>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiting_on_me: Option<WaitingOnMe>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub my_messages: Option<MyMessages>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub my_tickets: Option<MyTickets>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shipments: Option<ShipmentsInFlight>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receivables: Option<Receivables>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub people: Option<People>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_desk: Option<ServiceDesk>,
}

async fn my_work(conn: &mut PgConnection, me: Uuid) -> ApiResult<MyWork> {
    let (open, overdue): (i64, i64) = sqlx::query_as(
        "select count(*) filter (where status in ('open','in_progress','blocked')) as open,
                count(*) filter (where status in ('open','in_progress','blocked')
                                   and due_at < now()) as overdue
           from work_orders where assigned_to = $1",
    )
    .bind(me)
    .fetch_one(&mut *conn)
    .await?;
    let next: Vec<WorkOrderBrief> = sqlx::query_as(
        "select w.id, w.kind, w.title, w.status, w.due_at, s.reference as shipment_reference
           from work_orders w left join shipments s on s.id = w.shipment_id
          where w.assigned_to = $1 and w.status in ('open','in_progress','blocked')
          order by w.due_at nulls last, w.created_at
          limit 5",
    )
    .bind(me)
    .fetch_all(conn)
    .await?;
    Ok(MyWork {
        open,
        overdue,
        next,
    })
}

async fn my_leave(conn: &mut PgConnection, me: Uuid) -> ApiResult<MyLeave> {
    let row: MyLeave = sqlx::query_as(
        "select count(*) filter (where status = 'pending') as pending,
                count(*) filter (where status = 'approved' and end_date >= current_date)
                  as upcoming_approved,
                min(start_date) filter (where status in ('pending','approved')
                                          and end_date >= current_date) as next_start
           from leave_requests where employee_id = $1",
    )
    .bind(me)
    .fetch_one(conn)
    .await?;
    Ok(row)
}

async fn waiting_on_me(conn: &mut PgConnection, actor: &Actor) -> ApiResult<WaitingOnMe> {
    let leave_requests: i64 = if actor.has("leave:manage:all") {
        sqlx::query_scalar("select count(*) from leave_requests where status = 'pending'")
            .fetch_one(&mut *conn)
            .await?
    } else if actor.has("leave:approve:subtree") {
        sqlx::query_scalar(
            "select count(*) from leave_requests
              where status = 'pending' and current_approver_id = $1",
        )
        .bind(actor.me())
        .fetch_one(&mut *conn)
        .await?
    } else {
        0
    };
    // The two expense steps: managers clear `submitted` inside their subtree, finance
    // clears everything sitting at `manager_approved`.
    let mut expense_claims: i64 = 0;
    if actor.has("expenses:approve:subtree") {
        expense_claims += sqlx::query_scalar::<_, i64>(
            "select count(*) from expenses x join employees e on e.id = x.employee_id
              where x.status = 'submitted' and x.employee_id <> $1 and e.path <@ $2::ltree",
        )
        .bind(actor.me())
        .bind(actor.principal.path.clone())
        .fetch_one(&mut *conn)
        .await?;
    }
    if actor.has("expenses:approve:finance") {
        expense_claims += sqlx::query_scalar::<_, i64>(
            "select count(*) from expenses where status = 'manager_approved'",
        )
        .fetch_one(conn)
        .await?;
    }
    Ok(WaitingOnMe {
        leave_requests,
        expense_claims,
    })
}

async fn my_messages(conn: &mut PgConnection, me: Uuid) -> ApiResult<MyMessages> {
    let row: MyMessages = sqlx::query_as(
        "select count(*) as unread, count(distinct m.thread_id) as threads
           from thread_participants tp
           join messages m on m.thread_id = tp.thread_id
          where tp.employee_id = $1 and not tp.archived
            and m.sender_id is distinct from $1
            and (tp.last_read_at is null or m.sent_at > tp.last_read_at)",
    )
    .bind(me)
    .fetch_one(conn)
    .await?;
    Ok(row)
}

async fn my_tickets(conn: &mut PgConnection, me: Uuid) -> ApiResult<MyTickets> {
    let row: MyTickets = sqlx::query_as(
        "select count(*) filter (where requester_id = $1
                                   and status not in ('resolved','closed')) as open,
                count(*) filter (where assignee_id = $1
                                   and status not in ('resolved','closed')) as assigned_to_me,
                count(*) filter (where requester_id = $1 and status = 'resolved')
                  as awaiting_my_close
           from support_tickets where requester_id = $1 or assignee_id = $1",
    )
    .bind(me)
    .fetch_one(conn)
    .await?;
    Ok(row)
}

async fn shipments(conn: &mut PgConnection) -> ApiResult<ShipmentsInFlight> {
    let by_status: Vec<StatusCount> = sqlx::query_as(
        "select status, count(*) as count from shipments
          where status in ('booked','picked_up','in_transit','customs','out_for_delivery','exception')
          group by status order by status",
    )
    .fetch_all(conn)
    .await?;
    Ok(ShipmentsInFlight {
        total: by_status.iter().map(|s| s.count).sum(),
        by_status,
    })
}

async fn receivables(conn: &mut PgConnection) -> ApiResult<Receivables> {
    let row: Receivables = sqlx::query_as(
        "select coalesce(sum(total - amount_paid), 0) as outstanding,
                coalesce(sum(total - amount_paid) filter (where due_date < current_date), 0)
                  as overdue,
                count(*) as open_invoices,
                count(*) filter (where due_date < current_date) as overdue_invoices
           from invoices where status in ('issued','partially_paid')",
    )
    .fetch_one(conn)
    .await?;
    Ok(row)
}

async fn people(conn: &mut PgConnection) -> ApiResult<People> {
    let row: People = sqlx::query_as(
        "select count(*) filter (where status <> 'terminated') as headcount,
                (select count(*) from leave_requests l
                  where l.status = 'approved'
                    and current_date between l.start_date and l.end_date) as on_leave_today,
                count(*) filter (where hire_date >= date_trunc('month', current_date))
                  as joined_this_month
           from employees",
    )
    .fetch_one(conn)
    .await?;
    Ok(row)
}

async fn service_desk(conn: &mut PgConnection) -> ApiResult<ServiceDesk> {
    let row: ServiceDesk = sqlx::query_as(
        "select count(*) filter (where status not in ('resolved','closed')) as open,
                count(*) filter (where status not in ('resolved','closed')
                                   and assignee_id is null) as unassigned,
                count(*) filter (where status not in ('resolved','closed')
                                   and first_response_at is null and sla_due_at < now())
                  as breaching_sla
           from support_tickets",
    )
    .fetch_one(conn)
    .await?;
    Ok(row)
}

#[utoipa::path(get, path = "/api/v1/dashboard", tag = "dashboard", security(("bearer" = [])),
    responses((status = 200, body = Dashboard)))]
pub async fn dashboard(State(state): State<AppState>, actor: Actor) -> ApiResult<Json<Dashboard>> {
    let me = actor.me();
    let mut conn = state.pool.acquire().await?;
    let approver = actor.has("leave:approve:subtree")
        || actor.has("leave:manage:all")
        || actor.has("expenses:approve:subtree")
        || actor.has("expenses:approve:finance");
    let summary = Dashboard {
        employee_id: me,
        name: actor.principal.full_name(),
        title: actor.principal.title.clone(),
        roles: actor.principal.roles.clone(),
        generated_at: Utc::now(),
        my_work: match actor.has("tasks:read:self") {
            true => Some(my_work(&mut conn, me).await?),
            false => None,
        },
        my_leave: match actor.has("leave:request") {
            true => Some(my_leave(&mut conn, me).await?),
            false => None,
        },
        waiting_on_me: match approver {
            true => Some(waiting_on_me(&mut conn, &actor).await?),
            false => None,
        },
        my_messages: Some(my_messages(&mut conn, me).await?),
        my_tickets: match actor.has("tickets:create") {
            true => Some(my_tickets(&mut conn, me).await?),
            false => None,
        },
        shipments: match actor.has("shipments:read") {
            true => Some(shipments(&mut conn).await?),
            false => None,
        },
        receivables: match actor.has("ledger:read") {
            true => Some(receivables(&mut conn).await?),
            false => None,
        },
        people: match actor.has("employees:read:all") {
            true => Some(people(&mut conn).await?),
            false => None,
        },
        service_desk: match actor.has("tickets:read:all") || actor.has("tickets:manage") {
            true => Some(service_desk(&mut conn).await?),
            false => None,
        },
    };
    Ok(Json(summary))
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/dashboard", get(dashboard))
}

#[derive(OpenApi)]
#[openapi(paths(dashboard))]
pub struct DashboardApi;

#[cfg(test)]
mod tests {
    use super::*;

    /// Axum panics when two routes collide, and a router is cheap to assemble without
    /// any state, so the whole set is built once here.
    #[test]
    fn the_routers_merge_without_colliding() {
        let _: Router<AppState> = Router::new()
            .merge(crate::finance::handlers::routes())
            .merge(crate::comms::handlers::routes())
            .merge(crate::support::handlers::routes())
            .merge(crate::admin::handlers::routes())
            .merge(routes());
    }

    /// Every endpoint has to reach the published document, not just the router.
    #[test]
    fn the_document_carries_the_endpoints() {
        let mut doc = crate::finance::handlers::FinanceApi::openapi();
        doc.merge(crate::comms::handlers::CommsApi::openapi());
        doc.merge(crate::support::handlers::SupportApi::openapi());
        doc.merge(crate::admin::handlers::AdminApi::openapi());
        doc.merge(DashboardApi::openapi());
        for path in [
            "/api/v1/finance/accounts",
            "/api/v1/finance/periods/{id}/close",
            "/api/v1/finance/journal",
            "/api/v1/finance/journal/{id}/reverse",
            "/api/v1/finance/invoices",
            "/api/v1/finance/invoices/{id}/issue",
            "/api/v1/finance/payments",
            "/api/v1/finance/vendors",
            "/api/v1/finance/bills/{id}/pay",
            "/api/v1/finance/expenses/{id}/approve",
            "/api/v1/finance/payroll/runs/{id}/post",
            "/api/v1/finance/reports/ar-aging",
            "/api/v1/comms/recipients",
            "/api/v1/comms/threads/{id}",
            "/api/v1/comms/announcements",
            "/api/v1/support/tickets/{id}/assign",
            "/api/v1/admin/users/{id}/reset-password",
            "/api/v1/admin/audit",
            "/api/v1/dashboard",
        ] {
            assert!(doc.paths.paths.contains_key(path), "{path} is missing");
        }
    }
}
