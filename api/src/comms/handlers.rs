//! Inbox endpoints: who you may write to, threads, messages and announcements.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{PgConnection, Postgres, QueryBuilder};
use utoipa::{IntoParams, OpenApi, ToSchema};
use uuid::Uuid;
use validator::Validate;

use crate::audit;
use crate::auth::Actor;
use crate::comms::service::{self, MessageOut, ParticipantOut};
use crate::error::{ApiError, ApiResult, Problem};
use crate::http::extract::ValidatedJson;
use crate::http::pagination::{split_total, PageOut, PageQuery};
use crate::state::AppState;

const IMPORTANCE: &[&str] = &["low", "normal", "high"];

#[derive(Debug, Serialize, ToSchema)]
pub struct RecipientOut {
    pub id: Uuid,
    pub name: String,
    pub title: String,
    pub email: String,
    pub department_id: Uuid,
    pub department_name: String,
    /// `chain`, `department`, `subtree` or `any`: the rule that makes this person
    /// reachable.
    pub reason: String,
}

#[derive(sqlx::FromRow)]
struct RecipientRow {
    id: Uuid,
    name: String,
    title: String,
    email: String,
    department_id: Uuid,
    department_name: String,
    manager_id: Option<Uuid>,
    path: String,
}

#[utoipa::path(get, path = "/api/v1/comms/recipients", tag = "comms", security(("bearer" = [])),
    params(PageQuery), responses((status = 200, body = PageOut<RecipientOut>)))]
pub async fn recipients(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
) -> ApiResult<Json<PageOut<RecipientOut>>> {
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        "select e.id, e.first_name || ' ' || e.last_name as name, p.title, e.email::text as email,
                e.department_id, d.name as department_name, e.manager_id, e.path::text as path,
                count(*) over() as total_count
           from employees e
           join positions p on p.id = e.position_id
           join departments d on d.id = e.department_id
          where ",
    );
    service::push_messageable(&mut qb, &actor.principal, "e");
    if let Some(pattern) = page.search() {
        qb.push(" and (e.first_name || ' ' || e.last_name ilike ")
            .push_bind(pattern.clone())
            .push(" or p.title ilike ")
            .push_bind(pattern.clone())
            .push(" or e.email::text ilike ")
            .push_bind(pattern)
            .push(")");
    }
    qb.push(" order by e.last_name, e.first_name limit ");
    let paging = page.page();
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&state.pool).await?;
    let (rows, total) = split_total::<RecipientRow>(rows)?;
    let items = rows
        .into_iter()
        .map(|r| RecipientOut {
            reason: service::reason_for(
                &actor.principal,
                r.id,
                r.manager_id,
                r.department_id,
                &r.path,
            )
            .to_string(),
            id: r.id,
            name: r.name,
            title: r.title,
            email: r.email,
            department_id: r.department_id,
            department_name: r.department_name,
        })
        .collect();
    Ok(Json(PageOut::new(items, paging, total)))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct LastMessage {
    pub id: Uuid,
    pub sender_id: Option<Uuid>,
    pub sender_name: Option<String>,
    pub body: String,
    pub sent_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ThreadSummary {
    pub id: Uuid,
    pub kind: String,
    pub subject: String,
    pub created_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub last_message_at: DateTime<Utc>,
    pub my_role: String,
    pub archived: bool,
    pub last_read_at: Option<DateTime<Utc>>,
    pub unread_count: i64,
    pub participant_count: i64,
    pub last_message: Option<LastMessage>,
}

#[derive(sqlx::FromRow)]
struct ThreadRow {
    id: Uuid,
    kind: String,
    subject: String,
    created_by: Option<Uuid>,
    created_at: DateTime<Utc>,
    last_message_at: DateTime<Utc>,
    my_role: String,
    archived: bool,
    last_read_at: Option<DateTime<Utc>>,
    unread_count: i64,
    participant_count: i64,
    message_id: Option<Uuid>,
    message_sender_id: Option<Uuid>,
    message_sender_name: Option<String>,
    message_body: Option<String>,
    message_sent_at: Option<DateTime<Utc>>,
}

impl From<ThreadRow> for ThreadSummary {
    fn from(r: ThreadRow) -> ThreadSummary {
        let last_message = match (r.message_id, r.message_body, r.message_sent_at) {
            (Some(id), Some(body), Some(sent_at)) => Some(LastMessage {
                id,
                sender_id: r.message_sender_id,
                sender_name: r.message_sender_name,
                body,
                sent_at,
            }),
            _ => None,
        };
        ThreadSummary {
            id: r.id,
            kind: r.kind,
            subject: r.subject,
            created_by: r.created_by,
            created_at: r.created_at,
            last_message_at: r.last_message_at,
            my_role: r.my_role,
            archived: r.archived,
            last_read_at: r.last_read_at,
            unread_count: r.unread_count,
            participant_count: r.participant_count,
            last_message,
        }
    }
}

const THREAD_SELECT: &str = "select t.id, t.kind, t.subject, t.created_by, t.created_at, t.last_message_at,
                tp.role as my_role, tp.archived, tp.last_read_at,
                (select count(*) from messages m
                  where m.thread_id = t.id and m.sender_id is distinct from tp.employee_id
                    and (tp.last_read_at is null or m.sent_at > tp.last_read_at)) as unread_count,
                (select count(*) from thread_participants x where x.thread_id = t.id) as participant_count,
                lm.id as message_id, lm.sender_id as message_sender_id,
                s.first_name || ' ' || s.last_name as message_sender_name,
                lm.body as message_body, lm.sent_at as message_sent_at,
                count(*) over() as total_count
           from thread_participants tp
           join threads t on t.id = tp.thread_id
           left join lateral (select m.id, m.sender_id, m.body, m.sent_at from messages m
                               where m.thread_id = t.id order by m.sent_at desc, m.id desc limit 1) lm on true
           left join employees s on s.id = lm.sender_id
          where tp.employee_id = ";

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ThreadFilter {
    /// `direct`, `announcement` or `ticket`.
    pub kind: Option<String>,
    /// `1` returns only threads with unread messages.
    pub unread: Option<u8>,
    /// `1` returns the archived threads instead of the active ones.
    pub archived: Option<u8>,
}

#[utoipa::path(get, path = "/api/v1/comms/threads", tag = "comms", security(("bearer" = [])),
    params(PageQuery, ThreadFilter), responses((status = 200, body = PageOut<ThreadSummary>)))]
pub async fn list_threads(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<ThreadFilter>,
) -> ApiResult<Json<PageOut<ThreadSummary>>> {
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(THREAD_SELECT);
    qb.push_bind(actor.me());
    qb.push(" and tp.archived = ")
        .push_bind(filter.archived == Some(1));
    if let Some(kind) = &filter.kind {
        qb.push(" and t.kind = ").push_bind(kind.clone());
    }
    if filter.unread == Some(1) {
        qb.push(
            " and exists (select 1 from messages m where m.thread_id = t.id
                            and m.sender_id is distinct from tp.employee_id
                            and (tp.last_read_at is null or m.sent_at > tp.last_read_at)) ",
        );
    }
    if let Some(q) = page.search() {
        qb.push(" and (t.subject ilike ")
            .push_bind(q.clone())
            .push(" or exists (select 1 from messages m where m.thread_id = t.id and m.body ilike ")
            .push_bind(q)
            .push("))");
    }
    qb.push(" order by t.last_message_at desc limit ");
    let paging = page.page();
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&state.pool).await?;
    let (rows, total) = split_total::<ThreadRow>(rows)?;
    let items = rows.into_iter().map(ThreadSummary::from).collect();
    Ok(Json(PageOut::new(items, paging, total)))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ThreadDetail {
    #[serde(flatten)]
    pub thread: ThreadSummary,
    #[schema(value_type = Option<Object>)]
    pub audience: Option<serde_json::Value>,
    pub participants: Vec<ParticipantOut>,
    pub messages: Vec<MessageOut>,
}

async fn thread_summary(
    conn: &mut PgConnection,
    thread_id: Uuid,
    me: Uuid,
) -> ApiResult<ThreadSummary> {
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(THREAD_SELECT);
    qb.push_bind(me).push(" and t.id = ").push_bind(thread_id);
    let row: Option<ThreadRow> = qb.build_query_as().fetch_optional(conn).await?;
    row.map(ThreadSummary::from)
        .ok_or_else(|| ApiError::not_found("thread"))
}

async fn thread_detail(
    conn: &mut PgConnection,
    thread_id: Uuid,
    me: Uuid,
) -> ApiResult<ThreadDetail> {
    let thread = thread_summary(&mut *conn, thread_id, me).await?;
    let audience: Option<serde_json::Value> =
        sqlx::query_scalar("select audience from threads where id = $1")
            .bind(thread_id)
            .fetch_one(&mut *conn)
            .await?;
    let participants = service::thread_participants(&mut *conn, thread_id).await?;
    let messages = service::thread_messages(conn, thread_id).await?;
    Ok(ThreadDetail {
        thread,
        audience,
        participants,
        messages,
    })
}

#[utoipa::path(get, path = "/api/v1/comms/threads/{id}", tag = "comms", security(("bearer" = [])),
    responses((status = 200, body = ThreadDetail), (status = 404, body = Problem)))]
pub async fn get_thread(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<ThreadDetail>> {
    let mut tx = state.pool.begin().await?;
    // Opening a thread is what marks it read; a non-participant simply has no thread.
    let detail = thread_detail(&mut tx, id, actor.me()).await?;
    service::mark_read(&mut tx, id, actor.me()).await?;
    tx.commit().await?;
    Ok(Json(detail))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct NewThread {
    pub recipient_ids: Vec<Uuid>,
    #[validate(length(min = 1, max = 200))]
    pub subject: String,
    #[validate(length(min = 1, max = 20000))]
    pub body: String,
    /// `low`, `normal` or `high`.
    pub importance: Option<String>,
}

fn importance_of(value: &Option<String>) -> ApiResult<&str> {
    match value.as_deref() {
        None => Ok("normal"),
        Some(v) if IMPORTANCE.contains(&v) => Ok(v),
        Some(_) => Err(ApiError::validation(
            "importance",
            "must be low, normal or high",
        )),
    }
}

#[utoipa::path(post, path = "/api/v1/comms/threads", tag = "comms", security(("bearer" = [])),
    request_body = NewThread,
    responses((status = 201, body = ThreadDetail), (status = 403, body = Problem),
              (status = 422, body = Problem)))]
pub async fn create_thread(
    State(state): State<AppState>,
    actor: Actor,
    ValidatedJson(body): ValidatedJson<NewThread>,
) -> ApiResult<(StatusCode, Json<ThreadDetail>)> {
    let importance = importance_of(&body.importance)?.to_string();
    let mut recipients = body.recipient_ids.clone();
    recipients.sort();
    recipients.dedup();
    recipients.retain(|id| *id != actor.me());
    let mut tx = state.pool.begin().await?;
    service::assert_may_message(&mut tx, &actor.principal, &recipients).await?;
    let thread_id =
        service::create_thread(&mut tx, "direct", &body.subject, actor.me(), None).await?;
    service::add_participants(&mut tx, thread_id, &[actor.me()], "sender").await?;
    service::add_participants(&mut tx, thread_id, &recipients, "recipient").await?;
    let message_id =
        service::append_message(&mut tx, thread_id, actor.me(), &body.body, &importance).await?;
    let notified = service::notify(
        &mut tx,
        &recipients,
        &actor.principal.full_name(),
        &body.subject,
        &body.body,
    )
    .await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "thread.create",
        "thread",
        Some(thread_id),
        None,
        Some(json!({
            "kind": "direct",
            "subject": body.subject,
            "recipients": recipients,
            "message_id": message_id,
            "notifications": notified,
        })),
    )
    .await?;
    let detail = thread_detail(&mut tx, thread_id, actor.me()).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(detail)))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct NewMessage {
    #[validate(length(min = 1, max = 20000))]
    pub body: String,
    /// `low`, `normal` or `high`.
    pub importance: Option<String>,
}

#[utoipa::path(post, path = "/api/v1/comms/threads/{id}/messages", tag = "comms", security(("bearer" = [])),
    request_body = NewMessage,
    responses((status = 201, body = MessageOut), (status = 404, body = Problem),
              (status = 409, body = Problem)))]
pub async fn post_message(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<NewMessage>,
) -> ApiResult<(StatusCode, Json<MessageOut>)> {
    let importance = importance_of(&body.importance)?.to_string();
    let mut tx = state.pool.begin().await?;
    if !service::is_participant(&mut tx, id, actor.me()).await? {
        return Err(ApiError::not_found("thread"));
    }
    let row: (String, String) = sqlx::query_as("select kind, subject from threads where id = $1")
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    let (kind, subject) = row;
    if kind == "ticket" {
        return Err(ApiError::conflict(
            "reply to a support ticket through /support/tickets/{id}/messages",
        ));
    }
    let message_id =
        service::append_message(&mut tx, id, actor.me(), &body.body, &importance).await?;
    let recipients = service::other_participants(&mut tx, id, actor.me()).await?;
    let notified = service::notify(
        &mut tx,
        &recipients,
        &actor.principal.full_name(),
        &subject,
        &body.body,
    )
    .await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "message.send",
        "message",
        Some(message_id),
        None,
        Some(json!({
            "thread_id": id,
            "importance": importance,
            "recipients": recipients.len(),
            "notifications": notified,
        })),
    )
    .await?;
    let messages = service::thread_messages(&mut tx, id).await?;
    tx.commit().await?;
    let message = messages
        .into_iter()
        .find(|m| m.id == message_id)
        .ok_or_else(|| ApiError::internal_msg("the message vanished after it was written"))?;
    Ok((StatusCode::CREATED, Json(message)))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct NewAnnouncement {
    /// `company`, `department` or `subtree`.
    pub scope: String,
    /// Department id for `department`; ignored otherwise.
    #[serde(rename = "ref")]
    pub reference: Option<Uuid>,
    #[validate(length(min = 1, max = 200))]
    pub subject: String,
    #[validate(length(min = 1, max = 20000))]
    pub body: String,
    /// `low`, `normal` or `high`.
    pub importance: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AnnouncementOut {
    #[serde(flatten)]
    pub thread: ThreadDetail,
    pub audience_size: i64,
    pub notifications: i64,
}

/// Works out who hears an announcement and checks the caller is allowed to address
/// them. `department` has no permission of its own in the role model, so it is granted
/// by `messages:broadcast:company`, or by `messages:broadcast:subtree` when the target
/// department is the caller's own or one below it.
async fn announcement_audience(
    conn: &mut PgConnection,
    actor: &Actor,
    body: &NewAnnouncement,
) -> ApiResult<(Vec<Uuid>, serde_json::Value)> {
    match body.scope.as_str() {
        "company" => {
            actor.require("messages:broadcast:company")?;
            let ids: Vec<Uuid> = sqlx::query_scalar(
                "select id from employees where status <> 'terminated' and id <> $1",
            )
            .bind(actor.me())
            .fetch_all(conn)
            .await?;
            Ok((ids, json!({"scope": "company"})))
        }
        "subtree" => {
            actor.require("messages:broadcast:subtree")?;
            let ids: Vec<Uuid> = sqlx::query_scalar(
                "select id from employees
                  where status <> 'terminated' and path <@ $1::ltree and id <> $2",
            )
            .bind(actor.principal.path.clone())
            .bind(actor.me())
            .fetch_all(conn)
            .await?;
            Ok((ids, json!({"scope": "subtree", "ref": actor.me()})))
        }
        "department" => {
            let department = body.reference.unwrap_or(actor.principal.department_id);
            let allowed = actor.has("messages:broadcast:company")
                || (actor.has("messages:broadcast:subtree")
                    && actor.principal.department_ids.contains(&department));
            if !allowed {
                return Err(ApiError::forbidden(
                    "announcing to a department requires messages:broadcast:company, or \
                     messages:broadcast:subtree for your own department",
                ));
            }
            let ids: Vec<Uuid> = sqlx::query_scalar(
                "with recursive d as (
                    select id from departments where id = $1
                    union all
                    select c.id from departments c join d on c.parent_id = d.id)
                 select e.id from employees e
                  where e.status <> 'terminated' and e.department_id in (select id from d)
                    and e.id <> $2",
            )
            .bind(department)
            .bind(actor.me())
            .fetch_all(conn)
            .await?;
            Ok((ids, json!({"scope": "department", "ref": department})))
        }
        other => Err(ApiError::validation(
            "scope",
            format!("{other} is not one of company, department or subtree"),
        )),
    }
}

#[utoipa::path(post, path = "/api/v1/comms/announcements", tag = "comms", security(("bearer" = [])),
    request_body = NewAnnouncement,
    responses((status = 201, body = AnnouncementOut), (status = 403, body = Problem),
              (status = 422, body = Problem)))]
pub async fn create_announcement(
    State(state): State<AppState>,
    actor: Actor,
    ValidatedJson(body): ValidatedJson<NewAnnouncement>,
) -> ApiResult<(StatusCode, Json<AnnouncementOut>)> {
    let importance = importance_of(&body.importance)?.to_string();
    let mut tx = state.pool.begin().await?;
    let (audience, audience_json) = announcement_audience(&mut tx, &actor, &body).await?;
    if audience.is_empty() {
        return Err(ApiError::conflict("that audience has nobody in it"));
    }
    let thread_id = service::create_thread(
        &mut tx,
        "announcement",
        &body.subject,
        actor.me(),
        Some(audience_json.clone()),
    )
    .await?;
    service::add_participants(&mut tx, thread_id, &[actor.me()], "sender").await?;
    // The audience is resolved once, at send time: later joiners do not see it, and
    // people who leave keep the copy they were sent.
    service::add_participants(&mut tx, thread_id, &audience, "recipient").await?;
    let message_id =
        service::append_message(&mut tx, thread_id, actor.me(), &body.body, &importance).await?;
    let notified = service::notify(
        &mut tx,
        &audience,
        &actor.principal.full_name(),
        &body.subject,
        &body.body,
    )
    .await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "announcement.send",
        "thread",
        Some(thread_id),
        None,
        Some(json!({
            "audience": audience_json,
            "audience_size": audience.len(),
            "subject": body.subject,
            "message_id": message_id,
            "notifications": notified,
        })),
    )
    .await?;
    let detail = thread_detail(&mut tx, thread_id, actor.me()).await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(AnnouncementOut {
            thread: detail,
            audience_size: audience.len() as i64,
            notifications: notified as i64,
        }),
    ))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct ArchiveThread {
    /// `false` puts the thread back in the active inbox.
    pub archived: Option<bool>,
}

#[utoipa::path(post, path = "/api/v1/comms/threads/{id}/archive", tag = "comms", security(("bearer" = [])),
    request_body = ArchiveThread,
    responses((status = 200, body = ThreadSummary), (status = 404, body = Problem)))]
pub async fn archive_thread(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    body: Option<ValidatedJson<ArchiveThread>>,
) -> ApiResult<Json<ThreadSummary>> {
    let archived = body.and_then(|ValidatedJson(b)| b.archived).unwrap_or(true);
    let mut tx = state.pool.begin().await?;
    if !service::is_participant(&mut tx, id, actor.me()).await? {
        return Err(ApiError::not_found("thread"));
    }
    sqlx::query(
        "update thread_participants set archived = $3 where thread_id = $1 and employee_id = $2",
    )
    .bind(id)
    .bind(actor.me())
    .bind(archived)
    .execute(&mut *tx)
    .await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        if archived {
            "thread.archive"
        } else {
            "thread.unarchive"
        },
        "thread",
        Some(id),
        Some(json!({"archived": !archived})),
        Some(json!({"archived": archived})),
    )
    .await?;
    let summary = thread_summary(&mut tx, id, actor.me()).await?;
    tx.commit().await?;
    Ok(Json(summary))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/comms/recipients", get(recipients))
        .route("/comms/threads", get(list_threads).post(create_thread))
        .route("/comms/threads/:id", get(get_thread))
        .route("/comms/threads/:id/messages", post(post_message))
        .route("/comms/threads/:id/archive", post(archive_thread))
        .route("/comms/announcements", post(create_announcement))
}

#[derive(OpenApi)]
#[openapi(paths(
    recipients,
    list_threads,
    get_thread,
    create_thread,
    post_message,
    create_announcement,
    archive_thread
))]
pub struct CommsApi;
