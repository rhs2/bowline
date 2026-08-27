//! Service desk: tickets, their thread, triage, the SLA clock and satisfaction.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgConnection, Postgres, QueryBuilder};
use utoipa::{IntoParams, OpenApi, ToSchema};
use uuid::Uuid;
use validator::Validate;

use crate::audit;
use crate::auth::Actor;
use crate::comms::service::{self as comms, MessageOut, ParticipantOut};
use crate::error::{ApiError, ApiResult, Problem};
use crate::http::extract::ValidatedJson;
use crate::http::pagination::{split_total, PageOut, PageQuery};
use crate::org::service as org;
use crate::state::AppState;

const CATEGORIES: &[&str] = &["it", "hr", "payroll", "operations", "facilities", "other"];
const PRIORITIES: &[&str] = &["low", "normal", "high", "urgent"];
const STATUSES: &[&str] = &[
    "open",
    "triaged",
    "in_progress",
    "waiting_on_requester",
    "resolved",
    "closed",
];

/// Days a requester has to reopen a ticket that was resolved.
const REOPEN_WINDOW_DAYS: i64 = 7;

/// Time to first response promised for each priority.
fn sla_hours(priority: &str) -> i64 {
    match priority {
        "urgent" => 1,
        "high" => 4,
        "low" => 72,
        _ => 24,
    }
}

/// The lifecycle from DOMAIN.md. Reopening a resolved ticket is the one step that
/// goes backwards, and it is bounded by [`REOPEN_WINDOW_DAYS`] for requesters.
fn allowed_next(status: &str) -> &'static [&'static str] {
    match status {
        "open" => &["triaged"],
        "triaged" => &["in_progress", "waiting_on_requester"],
        "in_progress" => &["waiting_on_requester", "resolved"],
        "waiting_on_requester" => &["in_progress", "resolved"],
        "resolved" => &["closed", "in_progress"],
        _ => &[],
    }
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct TicketSummary {
    pub id: Uuid,
    pub ticket_no: String,
    pub thread_id: Uuid,
    pub subject: String,
    pub requester_id: Uuid,
    pub requester_name: String,
    pub category: String,
    pub priority: String,
    pub status: String,
    pub assignee_id: Option<Uuid>,
    pub assignee_name: Option<String>,
    pub sla_due_at: DateTime<Utc>,
    /// True while the ticket is still waiting for its first reply past the SLA.
    pub sla_breached: bool,
    pub first_response_at: Option<DateTime<Utc>>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub closed_at: Option<DateTime<Utc>>,
    pub satisfaction: Option<i16>,
    pub message_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct TicketDetail {
    #[serde(flatten)]
    pub ticket: TicketSummary,
    pub participants: Vec<ParticipantOut>,
    pub messages: Vec<MessageOut>,
}

const TICKET_SELECT: &str = "select t.id, t.ticket_no, t.thread_id, th.subject, t.requester_id,
                r.first_name || ' ' || r.last_name as requester_name, t.category, t.priority, t.status,
                t.assignee_id, a.first_name || ' ' || a.last_name as assignee_name, t.sla_due_at,
                (t.first_response_at is null and t.sla_due_at < now()
                 and t.status not in ('resolved','closed')) as sla_breached,
                t.first_response_at, t.resolved_at, t.closed_at, t.satisfaction,
                (select count(*) from messages m where m.thread_id = t.thread_id) as message_count,
                t.created_at, t.updated_at, count(*) over() as total_count
           from support_tickets t
           join threads th on th.id = t.thread_id
           join employees r on r.id = t.requester_id
           left join employees a on a.id = t.assignee_id
          where ";

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct TicketFilter {
    pub status: Option<String>,
    pub priority: Option<String>,
    pub category: Option<String>,
    pub assignee_id: Option<Uuid>,
    /// `1` limits the list to tickets the caller raised.
    pub mine: Option<u8>,
    /// `1` limits the list to tickets past their first response target.
    pub breaching: Option<u8>,
}

#[utoipa::path(get, path = "/api/v1/support/tickets", tag = "support", security(("bearer" = [])),
    params(PageQuery, TicketFilter),
    responses((status = 200, body = PageOut<TicketSummary>)))]
pub async fn list_tickets(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<TicketFilter>,
) -> ApiResult<Json<PageOut<TicketSummary>>> {
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(TICKET_SELECT);
    let agent = actor.has("tickets:read:all") || actor.has("tickets:manage");
    if agent && filter.mine != Some(1) {
        qb.push(" true ");
    } else {
        // Everyone else sees the tickets they raised and the ones assigned to them.
        qb.push(" (t.requester_id = ")
            .push_bind(actor.me())
            .push(" or t.assignee_id = ")
            .push_bind(actor.me())
            .push(") ");
        if filter.mine == Some(1) {
            qb.push(" and t.requester_id = ").push_bind(actor.me());
        }
    }
    if let Some(status) = &filter.status {
        qb.push(" and t.status = ").push_bind(status.clone());
    }
    if let Some(priority) = &filter.priority {
        qb.push(" and t.priority = ").push_bind(priority.clone());
    }
    if let Some(category) = &filter.category {
        qb.push(" and t.category = ").push_bind(category.clone());
    }
    if let Some(assignee) = filter.assignee_id {
        qb.push(" and t.assignee_id = ").push_bind(assignee);
    }
    if filter.breaching == Some(1) {
        qb.push(
            " and t.first_response_at is null and t.sla_due_at < now()
              and t.status not in ('resolved','closed') ",
        );
    }
    if let Some(q) = page.search() {
        qb.push(" and (th.subject ilike ")
            .push_bind(q.clone())
            .push(" or t.ticket_no ilike ")
            .push_bind(q)
            .push(")");
    }
    let order = page.order_by(&[
        ("created_at", "t.created_at"),
        ("sla_due_at", "t.sla_due_at"),
        ("priority", "t.priority"),
        ("status", "t.status"),
    ]);
    qb.push(format!(" order by {order} limit "));
    let paging = page.page();
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&state.pool).await?;
    let (items, total) = split_total::<TicketSummary>(rows)?;
    Ok(Json(PageOut::new(items, paging, total)))
}

async fn load_ticket(conn: &mut PgConnection, id: Uuid) -> ApiResult<TicketSummary> {
    sqlx::query_as(&format!("{TICKET_SELECT} t.id = $1"))
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(|| ApiError::not_found("ticket"))
}

/// Agents see every ticket; everyone else sees the ones they raised or were given.
/// Anything else is a 404 rather than a 403.
fn assert_visible(actor: &Actor, ticket: &TicketSummary) -> ApiResult<()> {
    let visible = actor.has("tickets:read:all")
        || actor.has("tickets:manage")
        || ticket.requester_id == actor.me()
        || ticket.assignee_id == Some(actor.me());
    if visible {
        Ok(())
    } else {
        Err(ApiError::not_found("ticket"))
    }
}

async fn ticket_detail(conn: &mut PgConnection, id: Uuid) -> ApiResult<TicketDetail> {
    let ticket = load_ticket(&mut *conn, id).await?;
    let participants = comms::thread_participants(&mut *conn, ticket.thread_id).await?;
    let messages = comms::thread_messages(conn, ticket.thread_id).await?;
    Ok(TicketDetail {
        ticket,
        participants,
        messages,
    })
}

#[utoipa::path(get, path = "/api/v1/support/tickets/{id}", tag = "support", security(("bearer" = [])),
    responses((status = 200, body = TicketDetail), (status = 404, body = Problem)))]
pub async fn get_ticket(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<TicketDetail>> {
    let mut tx = state.pool.begin().await?;
    let detail = ticket_detail(&mut tx, id).await?;
    assert_visible(&actor, &detail.ticket)?;
    comms::mark_read(&mut tx, detail.ticket.thread_id, actor.me()).await?;
    tx.commit().await?;
    Ok(Json(detail))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct NewTicket {
    /// `it`, `hr`, `payroll`, `operations`, `facilities` or `other`.
    pub category: String,
    /// `low`, `normal`, `high` or `urgent`; defaults to `normal`.
    pub priority: Option<String>,
    #[validate(length(min = 1, max = 200))]
    pub subject: String,
    #[validate(length(min = 1, max = 20000))]
    pub body: String,
}

/// Active employees who can work the queue, used as the default audience of a new
/// ticket so it reaches the desk rather than one named person.
async fn desk_agents(conn: &mut PgConnection, exclude: Uuid) -> ApiResult<Vec<Uuid>> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "select u.employee_id from users u
           join user_permissions up on up.user_id = u.id
           join employees e on e.id = u.employee_id
          where up.permission_key = 'tickets:manage' and u.status = 'active'
            and e.status <> 'terminated' and u.employee_id <> $1",
    )
    .bind(exclude)
    .fetch_all(conn)
    .await?;
    Ok(ids)
}

#[utoipa::path(post, path = "/api/v1/support/tickets", tag = "support", security(("bearer" = [])),
    request_body = NewTicket,
    responses((status = 201, body = TicketDetail), (status = 422, body = Problem)))]
pub async fn create_ticket(
    State(state): State<AppState>,
    actor: Actor,
    ValidatedJson(body): ValidatedJson<NewTicket>,
) -> ApiResult<(StatusCode, Json<TicketDetail>)> {
    actor.require("tickets:create")?;
    if !CATEGORIES.contains(&body.category.as_str()) {
        return Err(ApiError::validation(
            "category",
            format!("must be one of {}", CATEGORIES.join(", ")),
        ));
    }
    let priority = body
        .priority
        .clone()
        .unwrap_or_else(|| "normal".to_string());
    if !PRIORITIES.contains(&priority.as_str()) {
        return Err(ApiError::validation(
            "priority",
            format!("must be one of {}", PRIORITIES.join(", ")),
        ));
    }
    let sla_due_at = Utc::now() + Duration::hours(sla_hours(&priority));
    let mut tx = state.pool.begin().await?;
    let thread_id = comms::create_thread(
        &mut tx,
        "ticket",
        &body.subject,
        actor.me(),
        Some(json!({"scope": "ticket"})),
    )
    .await?;
    let ticket_no = org::next_ticket_no(&mut tx).await?;
    let id: Uuid = sqlx::query_scalar(
        "insert into support_tickets (ticket_no, thread_id, requester_id, category, priority, sla_due_at)
         values ($1, $2, $3, $4, $5, $6) returning id",
    )
    .bind(&ticket_no)
    .bind(thread_id)
    .bind(actor.me())
    .bind(&body.category)
    .bind(&priority)
    .bind(sla_due_at)
    .fetch_one(&mut *tx)
    .await?;
    comms::add_participants(&mut tx, thread_id, &[actor.me()], "sender").await?;
    let agents = desk_agents(&mut tx, actor.me()).await?;
    comms::add_participants(&mut tx, thread_id, &agents, "agent").await?;
    comms::append_message(&mut tx, thread_id, actor.me(), &body.body, "normal").await?;
    let notified = comms::notify(
        &mut tx,
        &agents,
        &actor.principal.full_name(),
        &format!("[{ticket_no}] {}", body.subject),
        &body.body,
    )
    .await?;
    let after = audit::snapshot(&mut tx, "support_tickets", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "ticket.create",
        "ticket",
        Some(id),
        None,
        after.map(|t| json!({"ticket": t, "subject": body.subject, "notifications": notified})),
    )
    .await?;
    let detail = ticket_detail(&mut tx, id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(detail)))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct TicketMessage {
    #[validate(length(min = 1, max = 20000))]
    pub body: String,
}

#[utoipa::path(post, path = "/api/v1/support/tickets/{id}/messages", tag = "support",
    security(("bearer" = [])), request_body = TicketMessage,
    responses((status = 201, body = MessageOut), (status = 404, body = Problem),
              (status = 409, body = Problem)))]
pub async fn post_ticket_message(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<TicketMessage>,
) -> ApiResult<(StatusCode, Json<MessageOut>)> {
    let mut tx = state.pool.begin().await?;
    let ticket = load_ticket(&mut tx, id).await?;
    assert_visible(&actor, &ticket)?;
    if ticket.status == "closed" {
        return Err(ApiError::conflict(
            "this ticket is closed; raise a new one to carry on",
        ));
    }
    // An agent joining the conversation becomes a participant of the thread.
    if actor.has("tickets:manage")
        && !comms::is_participant(&mut tx, ticket.thread_id, actor.me()).await?
    {
        comms::add_participants(&mut tx, ticket.thread_id, &[actor.me()], "agent").await?;
    }
    let message_id =
        comms::append_message(&mut tx, ticket.thread_id, actor.me(), &body.body, "normal").await?;
    // The SLA clock stops on the first reply from the desk, never on the requester's
    // own follow-up.
    let first_response = ticket.first_response_at.is_none()
        && actor.me() != ticket.requester_id
        && actor.has("tickets:manage");
    if first_response {
        sqlx::query("update support_tickets set first_response_at = now() where id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    let recipients = comms::other_participants(&mut tx, ticket.thread_id, actor.me()).await?;
    let notified = comms::notify(
        &mut tx,
        &recipients,
        &actor.principal.full_name(),
        &format!("[{}] {}", ticket.ticket_no, ticket.subject),
        &body.body,
    )
    .await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "ticket.message",
        "ticket",
        Some(id),
        None,
        Some(json!({
            "message_id": message_id,
            "thread_id": ticket.thread_id,
            "first_response": first_response,
            "notifications": notified,
        })),
    )
    .await?;
    let messages = comms::thread_messages(&mut tx, ticket.thread_id).await?;
    tx.commit().await?;
    let message = messages
        .into_iter()
        .find(|m| m.id == message_id)
        .ok_or_else(|| ApiError::internal_msg("the message vanished after it was written"))?;
    Ok((StatusCode::CREATED, Json(message)))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct AssignTicket {
    pub assignee_id: Uuid,
}

#[utoipa::path(post, path = "/api/v1/support/tickets/{id}/assign", tag = "support",
    security(("bearer" = [])), request_body = AssignTicket,
    responses((status = 200, body = TicketDetail), (status = 403, body = Problem),
              (status = 409, body = Problem)))]
pub async fn assign_ticket(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<AssignTicket>,
) -> ApiResult<Json<TicketDetail>> {
    actor.require("tickets:manage")?;
    let mut tx = state.pool.begin().await?;
    let ticket = load_ticket(&mut tx, id).await?;
    if ["resolved", "closed"].contains(&ticket.status.as_str()) {
        return Err(ApiError::transition(&ticket.status, "triaged"));
    }
    let assignee: Option<String> = sqlx::query_scalar("select status from employees where id = $1")
        .bind(body.assignee_id)
        .fetch_optional(&mut *tx)
        .await?;
    match assignee.as_deref() {
        Some("terminated") | None => {
            return Err(ApiError::validation("assignee_id", "unknown employee"))
        }
        _ => {}
    }
    let before = audit::snapshot(&mut tx, "support_tickets", id).await?;
    // Assignment is the triage step: an open ticket now has an owner.
    let next = if ticket.status == "open" {
        "triaged".to_string()
    } else {
        ticket.status.clone()
    };
    sqlx::query("update support_tickets set assignee_id = $2, status = $3 where id = $1")
        .bind(id)
        .bind(body.assignee_id)
        .bind(&next)
        .execute(&mut *tx)
        .await?;
    comms::add_participants(&mut tx, ticket.thread_id, &[body.assignee_id], "agent").await?;
    let notified = comms::notify(
        &mut tx,
        &[body.assignee_id],
        &actor.principal.full_name(),
        &format!("[{}] {}", ticket.ticket_no, ticket.subject),
        &format!(
            "This ticket has been assigned to you. Priority {}, response due {}.",
            ticket.priority,
            ticket.sla_due_at.to_rfc3339()
        ),
    )
    .await?;
    let after = audit::snapshot(&mut tx, "support_tickets", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "ticket.assign",
        "ticket",
        Some(id),
        before,
        after.map(|t| json!({"ticket": t, "notifications": notified})),
    )
    .await?;
    let detail = ticket_detail(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(detail))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct TicketStatus {
    /// `triaged`, `in_progress`, `waiting_on_requester`, `resolved` or `closed`.
    pub status: String,
    #[validate(length(min = 1, max = 2000))]
    pub note: Option<String>,
}

/// Requesters may only close a resolved ticket, or reopen one inside the window.
fn assert_may_change(actor: &Actor, ticket: &TicketSummary, next: &str) -> ApiResult<()> {
    if actor.has("tickets:manage") {
        return Ok(());
    }
    if ticket.requester_id != actor.me() {
        return Err(ApiError::forbidden(
            "only the requester or an agent may do that",
        ));
    }
    match (ticket.status.as_str(), next) {
        ("resolved", "closed") => Ok(()),
        ("resolved", "in_progress") => {
            let resolved_at = ticket
                .resolved_at
                .ok_or_else(|| ApiError::conflict("this ticket has no resolution time"))?;
            if Utc::now() - resolved_at > Duration::days(REOPEN_WINDOW_DAYS) {
                Err(ApiError::conflict(format!(
                    "a resolved ticket can only be reopened within {REOPEN_WINDOW_DAYS} days"
                )))
            } else {
                Ok(())
            }
        }
        _ => Err(ApiError::forbidden(
            "a requester may close a resolved ticket or reopen it, nothing else",
        )),
    }
}

#[utoipa::path(post, path = "/api/v1/support/tickets/{id}/status", tag = "support",
    security(("bearer" = [])), request_body = TicketStatus,
    responses((status = 200, body = TicketDetail), (status = 403, body = Problem),
              (status = 409, body = Problem)))]
pub async fn set_ticket_status(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<TicketStatus>,
) -> ApiResult<Json<TicketDetail>> {
    if !STATUSES.contains(&body.status.as_str()) {
        return Err(ApiError::validation(
            "status",
            format!("must be one of {}", STATUSES.join(", ")),
        ));
    }
    let mut tx = state.pool.begin().await?;
    let ticket = load_ticket(&mut tx, id).await?;
    assert_visible(&actor, &ticket)?;
    if !allowed_next(&ticket.status).contains(&body.status.as_str()) {
        return Err(ApiError::transition(&ticket.status, &body.status));
    }
    assert_may_change(&actor, &ticket, &body.status)?;
    if body.status == "triaged" && ticket.assignee_id.is_none() {
        return Err(ApiError::conflict(
            "assign the ticket to someone before triaging it",
        ));
    }
    let before = audit::snapshot(&mut tx, "support_tickets", id).await?;
    sqlx::query(
        "update support_tickets
            set status = $2,
                resolved_at = case when $2 = 'resolved' then now()
                                   when $2 = 'in_progress' then null
                                   else resolved_at end,
                closed_at = case when $2 = 'closed' then now() else null end
          where id = $1",
    )
    .bind(id)
    .bind(&body.status)
    .execute(&mut *tx)
    .await?;
    if let Some(note) = &body.note {
        comms::append_message(&mut tx, ticket.thread_id, actor.me(), note, "normal").await?;
    }
    let recipients = comms::other_participants(&mut tx, ticket.thread_id, actor.me()).await?;
    let notified = comms::notify(
        &mut tx,
        &recipients,
        &actor.principal.full_name(),
        &format!("[{}] {}", ticket.ticket_no, ticket.subject),
        &match &body.note {
            Some(note) => format!("Ticket moved to {}.\n\n{note}", body.status),
            None => format!("Ticket moved to {}.", body.status),
        },
    )
    .await?;
    let after = audit::snapshot(&mut tx, "support_tickets", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "ticket.status",
        "ticket",
        Some(id),
        before,
        after.map(|t| json!({"ticket": t, "note": body.note, "notifications": notified})),
    )
    .await?;
    let detail = ticket_detail(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(detail))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct RateTicket {
    #[validate(range(min = 1, max = 5))]
    pub satisfaction: i16,
    #[validate(length(min = 1, max = 2000))]
    pub comment: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/support/tickets/{id}/rate", tag = "support",
    security(("bearer" = [])), request_body = RateTicket,
    responses((status = 200, body = TicketDetail), (status = 403, body = Problem),
              (status = 409, body = Problem)))]
pub async fn rate_ticket(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<RateTicket>,
) -> ApiResult<Json<TicketDetail>> {
    let mut tx = state.pool.begin().await?;
    let ticket = load_ticket(&mut tx, id).await?;
    if ticket.requester_id != actor.me() {
        return Err(ApiError::forbidden("only the requester may rate a ticket"));
    }
    if !["resolved", "closed"].contains(&ticket.status.as_str()) {
        return Err(ApiError::InvalidTransition(format!(
            "a ticket can only be rated once it is resolved, this one is {}",
            ticket.status
        )));
    }
    let before = audit::snapshot(&mut tx, "support_tickets", id).await?;
    sqlx::query("update support_tickets set satisfaction = $2 where id = $1")
        .bind(id)
        .bind(body.satisfaction)
        .execute(&mut *tx)
        .await?;
    if let Some(comment) = &body.comment {
        comms::append_message(&mut tx, ticket.thread_id, actor.me(), comment, "normal").await?;
    }
    let after = audit::snapshot(&mut tx, "support_tickets", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "ticket.rate",
        "ticket",
        Some(id),
        before,
        after,
    )
    .await?;
    let detail = ticket_detail(&mut tx, id).await?;
    tx.commit().await?;
    Ok(Json(detail))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/support/tickets", get(list_tickets).post(create_ticket))
        .route("/support/tickets/:id", get(get_ticket))
        .route("/support/tickets/:id/messages", post(post_ticket_message))
        .route("/support/tickets/:id/assign", post(assign_ticket))
        .route("/support/tickets/:id/status", post(set_ticket_status))
        .route("/support/tickets/:id/rate", post(rate_ticket))
}

#[derive(OpenApi)]
#[openapi(paths(
    list_tickets,
    get_ticket,
    create_ticket,
    post_ticket_message,
    assign_ticket,
    set_ticket_status,
    rate_ticket
))]
pub struct SupportApi;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sla_matches_the_published_targets() {
        assert_eq!(sla_hours("urgent"), 1);
        assert_eq!(sla_hours("high"), 4);
        assert_eq!(sla_hours("normal"), 24);
        assert_eq!(sla_hours("low"), 72);
    }

    #[test]
    fn the_lifecycle_only_moves_forward_or_reopens() {
        assert_eq!(allowed_next("open"), &["triaged"]);
        assert!(allowed_next("in_progress").contains(&"resolved"));
        assert!(allowed_next("resolved").contains(&"closed"));
        assert!(allowed_next("resolved").contains(&"in_progress"));
        assert!(allowed_next("closed").is_empty());
        assert!(!allowed_next("open").contains(&"resolved"));
    }
}
