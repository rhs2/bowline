//! Transactional email outbox: one `notifications` row per recipient, written in the
//! same transaction as the message that caused it. The notify worker delivers them.

use sqlx::PgConnection;
use uuid::Uuid;

pub async fn enqueue_email(
    conn: &mut PgConnection,
    recipient_ids: &[Uuid],
    subject: &str,
    body: &str,
) -> Result<u64, sqlx::Error> {
    if recipient_ids.is_empty() {
        return Ok(0);
    }
    let result = sqlx::query(
        "insert into notifications (recipient_id, channel, to_address, subject, body_text)
         select e.id, 'email', e.email, $1, $2
           from employees e
          where e.id = any($3) and e.status <> 'terminated'",
    )
    .bind(subject)
    .bind(body)
    .bind(recipient_ids)
    .execute(conn)
    .await?;
    Ok(result.rows_affected())
}
