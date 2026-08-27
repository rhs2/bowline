//! Messaging rules and the thread plumbing shared by the inbox and the service desk.
//!
//! Who may write to whom is decided here and nowhere else, from the permissions in
//! DOMAIN.md: the chain of command, the caller's department, the caller's subtree, or
//! anyone at all for support agents and executives. Everything that appends to a
//! thread also writes one outbox notification per recipient in the same transaction.

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgConnection, Postgres, QueryBuilder};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::auth::principal::Principal;
use crate::error::{ApiError, ApiResult};
use crate::outbox;

/// Largest audience a single direct thread may address.
pub const MAX_DIRECT_RECIPIENTS: usize = 50;

/// Appends the "may the caller message this employee" predicate for an `employees`
/// table aliased `alias`. The caller is always excluded, as are leavers.
pub fn push_messageable(qb: &mut QueryBuilder<'_, Postgres>, principal: &Principal, alias: &str) {
    qb.push(format!(
        " {alias}.status <> 'terminated' and {alias}.id <> "
    ));
    qb.push_bind(principal.employee_id);
    if principal.has("messages:send:any") {
        return;
    }
    qb.push(" and (false ");
    if principal.has("messages:send:chain") {
        qb.push(format!(" or {alias}.manager_id = "))
            .push_bind(principal.employee_id);
        if let Some(manager) = principal.manager_id {
            qb.push(format!(" or {alias}.id = ")).push_bind(manager);
        }
    }
    if principal.has("messages:send:department") {
        qb.push(format!(" or {alias}.department_id = "))
            .push_bind(principal.department_id);
    }
    if principal.has("messages:send:subtree") {
        qb.push(format!(" or {alias}.path <@ "))
            .push_bind(principal.path.clone())
            .push("::ltree ");
    }
    qb.push(") ");
}

/// Why a person is reachable, so the client can group the picker.
pub fn reason_for(
    principal: &Principal,
    target_id: Uuid,
    manager_id: Option<Uuid>,
    department_id: Uuid,
    path: &str,
) -> &'static str {
    if manager_id == Some(principal.employee_id) || principal.manager_id == Some(target_id) {
        "chain"
    } else if department_id == principal.department_id {
        "department"
    } else if principal.is_strictly_below(path) {
        "subtree"
    } else {
        "any"
    }
}

/// Refuses the whole request unless every recipient is reachable under the rules.
pub async fn assert_may_message(
    conn: &mut PgConnection,
    principal: &Principal,
    recipients: &[Uuid],
) -> ApiResult<()> {
    if recipients.is_empty() {
        return Err(ApiError::validation(
            "recipient_ids",
            "at least one recipient is required",
        ));
    }
    if recipients.len() > MAX_DIRECT_RECIPIENTS {
        return Err(ApiError::validation(
            "recipient_ids",
            format!("at most {MAX_DIRECT_RECIPIENTS} recipients; use an announcement instead"),
        ));
    }
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new("select e.id from employees e where ");
    push_messageable(&mut qb, principal, "e");
    qb.push(" and e.id = any(")
        .push_bind(recipients.to_vec())
        .push(")");
    let allowed: Vec<Uuid> = qb.build_query_scalar().fetch_all(conn).await?;
    if let Some(missing) = recipients.iter().find(|id| !allowed.contains(id)) {
        return Err(ApiError::forbidden(format!(
            "you are not allowed to message {missing}"
        )));
    }
    Ok(())
}

pub async fn create_thread(
    conn: &mut PgConnection,
    kind: &str,
    subject: &str,
    created_by: Uuid,
    audience: Option<serde_json::Value>,
) -> ApiResult<Uuid> {
    let id: Uuid = sqlx::query_scalar(
        "insert into threads (kind, subject, created_by, audience) values ($1, $2, $3, $4) returning id",
    )
    .bind(kind)
    .bind(subject)
    .bind(created_by)
    .bind(audience)
    .fetch_one(conn)
    .await?;
    Ok(id)
}

/// Adds participants, leaving any existing row (and its read marker) alone.
pub async fn add_participants(
    conn: &mut PgConnection,
    thread_id: Uuid,
    employee_ids: &[Uuid],
    role: &str,
) -> ApiResult<u64> {
    if employee_ids.is_empty() {
        return Ok(0);
    }
    let affected = sqlx::query(
        "insert into thread_participants (thread_id, employee_id, role)
         select $1, id, $3 from unnest($2::uuid[]) as t(id)
         on conflict (thread_id, employee_id) do nothing",
    )
    .bind(thread_id)
    .bind(employee_ids)
    .bind(role)
    .execute(conn)
    .await?
    .rows_affected();
    Ok(affected)
}

pub async fn is_participant(
    conn: &mut PgConnection,
    thread_id: Uuid,
    employee_id: Uuid,
) -> ApiResult<bool> {
    let found: Option<Uuid> = sqlx::query_scalar(
        "select employee_id from thread_participants where thread_id = $1 and employee_id = $2",
    )
    .bind(thread_id)
    .bind(employee_id)
    .fetch_optional(conn)
    .await?;
    Ok(found.is_some())
}

/// Everyone on the thread except the sender.
pub async fn other_participants(
    conn: &mut PgConnection,
    thread_id: Uuid,
    sender_id: Uuid,
) -> ApiResult<Vec<Uuid>> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "select employee_id from thread_participants where thread_id = $1 and employee_id <> $2",
    )
    .bind(thread_id)
    .bind(sender_id)
    .fetch_all(conn)
    .await?;
    Ok(ids)
}

/// Appends a message and moves the thread to the top of every inbox.
pub async fn append_message(
    conn: &mut PgConnection,
    thread_id: Uuid,
    sender_id: Uuid,
    body: &str,
    importance: &str,
) -> ApiResult<Uuid> {
    let id: Uuid = sqlx::query_scalar(
        "insert into messages (thread_id, sender_id, body, importance) values ($1, $2, $3, $4)
         returning id",
    )
    .bind(thread_id)
    .bind(sender_id)
    .bind(body)
    .bind(importance)
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query("update threads set last_message_at = now() where id = $1")
        .bind(thread_id)
        .execute(&mut *conn)
        .await?;
    // The sender has read their own message by definition.
    sqlx::query(
        "update thread_participants set last_read_at = now(), archived = false
          where thread_id = $1 and employee_id = $2",
    )
    .bind(thread_id)
    .bind(sender_id)
    .execute(conn)
    .await?;
    Ok(id)
}

/// One email notification per recipient, in the caller's transaction.
pub async fn notify(
    conn: &mut PgConnection,
    recipients: &[Uuid],
    sender_name: &str,
    subject: &str,
    body: &str,
) -> ApiResult<u64> {
    let text = format!("{sender_name} wrote:\n\n{body}");
    Ok(outbox::enqueue_email(conn, recipients, subject, &text).await?)
}

pub async fn mark_read(
    conn: &mut PgConnection,
    thread_id: Uuid,
    employee_id: Uuid,
) -> ApiResult<()> {
    sqlx::query(
        "update thread_participants set last_read_at = now() where thread_id = $1 and employee_id = $2",
    )
    .bind(thread_id)
    .bind(employee_id)
    .execute(conn)
    .await?;
    Ok(())
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct MessageOut {
    pub id: Uuid,
    pub thread_id: Uuid,
    pub sender_id: Option<Uuid>,
    pub sender_name: Option<String>,
    pub body: String,
    pub importance: String,
    pub sent_at: DateTime<Utc>,
}

pub async fn thread_messages(
    conn: &mut PgConnection,
    thread_id: Uuid,
) -> ApiResult<Vec<MessageOut>> {
    let rows: Vec<MessageOut> = sqlx::query_as(
        "select m.id, m.thread_id, m.sender_id,
                s.first_name || ' ' || s.last_name as sender_name, m.body, m.importance, m.sent_at
           from messages m left join employees s on s.id = m.sender_id
          where m.thread_id = $1 order by m.sent_at, m.id",
    )
    .bind(thread_id)
    .fetch_all(conn)
    .await?;
    Ok(rows)
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct ParticipantOut {
    pub employee_id: Uuid,
    pub name: String,
    pub title: String,
    pub role: String,
    pub archived: bool,
    pub last_read_at: Option<DateTime<Utc>>,
}

pub async fn thread_participants(
    conn: &mut PgConnection,
    thread_id: Uuid,
) -> ApiResult<Vec<ParticipantOut>> {
    let rows: Vec<ParticipantOut> = sqlx::query_as(
        "select p.employee_id, e.first_name || ' ' || e.last_name as name, po.title, p.role,
                p.archived, p.last_read_at
           from thread_participants p
           join employees e on e.id = p.employee_id
           join positions po on po.id = e.position_id
          where p.thread_id = $1
          order by p.role, e.last_name, e.first_name",
    )
    .bind(thread_id)
    .fetch_all(conn)
    .await?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    const ME: Uuid = Uuid::from_u128(1);
    const MY_MANAGER: Uuid = Uuid::from_u128(2);
    const MY_REPORT: Uuid = Uuid::from_u128(3);
    const COLLEAGUE: Uuid = Uuid::from_u128(4);
    const STRANGER: Uuid = Uuid::from_u128(5);
    const MY_DEPARTMENT: Uuid = Uuid::from_u128(10);
    const OTHER_DEPARTMENT: Uuid = Uuid::from_u128(11);
    const MY_PATH: &str = "ceo.boss.me";

    fn principal(permissions: &[&str]) -> Principal {
        Principal {
            user_id: Uuid::from_u128(100),
            employee_id: ME,
            email: "me@bowline.test".to_string(),
            user_status: "active".to_string(),
            token_version: 1,
            must_change_password: false,
            first_name: "Mo".to_string(),
            last_name: "Rivera".to_string(),
            title: "Freight Coordinator".to_string(),
            level: 6,
            position_id: Uuid::from_u128(20),
            department_id: MY_DEPARTMENT,
            department_ids: vec![MY_DEPARTMENT],
            manager_id: Some(MY_MANAGER),
            path: MY_PATH.to_string(),
            employee_status: "active".to_string(),
            roles: vec!["baseline".to_string()],
            permissions: permissions
                .iter()
                .map(|p| p.to_string())
                .collect::<HashSet<_>>(),
        }
    }

    #[test]
    fn the_reason_follows_the_closest_rule() {
        let p = principal(&["messages:send:chain", "messages:send:department"]);
        assert_eq!(
            reason_for(&p, MY_MANAGER, None, OTHER_DEPARTMENT, "ceo.boss"),
            "chain"
        );
        assert_eq!(
            reason_for(
                &p,
                MY_REPORT,
                Some(ME),
                OTHER_DEPARTMENT,
                "ceo.boss.me.report"
            ),
            "chain"
        );
        assert_eq!(
            reason_for(
                &p,
                COLLEAGUE,
                Some(MY_MANAGER),
                MY_DEPARTMENT,
                "ceo.boss.mate"
            ),
            "department"
        );
        assert_eq!(
            reason_for(
                &p,
                STRANGER,
                Some(MY_REPORT),
                OTHER_DEPARTMENT,
                "ceo.boss.me.report.deeper"
            ),
            "subtree"
        );
        assert_eq!(
            reason_for(
                &p,
                STRANGER,
                Some(COLLEAGUE),
                OTHER_DEPARTMENT,
                "ceo.elsewhere"
            ),
            "any"
        );
    }

    #[test]
    fn the_baseline_predicate_covers_the_chain_and_the_department() {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new("select e.id from employees e where ");
        push_messageable(
            &mut qb,
            &principal(&["messages:send:chain", "messages:send:department"]),
            "e",
        );
        let sql = qb.sql();
        assert!(sql.contains("e.status <> 'terminated'"));
        assert!(sql.contains("e.manager_id ="), "the chain rule is missing");
        assert!(
            sql.contains("e.department_id ="),
            "the department rule is missing"
        );
        assert!(!sql.contains("path <@"), "subtree was not granted");
    }

    #[test]
    fn send_any_needs_no_relationship_test() {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new("select e.id from employees e where ");
        push_messageable(&mut qb, &principal(&["messages:send:any"]), "e");
        let sql = qb.sql();
        assert!(sql.contains("e.id <>"), "the caller is still excluded");
        assert!(!sql.contains("department_id"));
        assert!(!sql.contains("manager_id"));
    }

    #[test]
    fn a_principal_with_no_messaging_permission_reaches_nobody() {
        let mut qb: QueryBuilder<Postgres> =
            QueryBuilder::new("select e.id from employees e where ");
        push_messageable(&mut qb, &principal(&[]), "e");
        assert!(
            qb.sql().contains("(false "),
            "the predicate must fall through to false"
        );
    }
}
