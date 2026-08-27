//! Append-only audit trail, written inside the same transaction as the change.

use serde_json::Value;
use sqlx::PgConnection;
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
pub struct AuditCtx {
    pub user_id: Option<Uuid>,
    pub employee_id: Option<Uuid>,
    pub ip: Option<String>,
    pub request_id: Option<String>,
}

pub async fn record(
    conn: &mut PgConnection,
    ctx: &AuditCtx,
    action: &str,
    entity_type: &str,
    entity_id: Option<Uuid>,
    before: Option<Value>,
    after: Option<Value>,
) -> Result<(), sqlx::Error> {
    let ip = ctx
        .ip
        .as_deref()
        .filter(|ip| ip.parse::<std::net::IpAddr>().is_ok());
    sqlx::query(
        "insert into audit_log (actor_user_id, actor_employee_id, action, entity_type, entity_id,
                                before, after, ip, request_id)
         values ($1, $2, $3, $4, $5, $6, $7, $8::inet, $9)",
    )
    .bind(ctx.user_id)
    .bind(ctx.employee_id)
    .bind(action)
    .bind(entity_type)
    .bind(entity_id)
    .bind(before)
    .bind(after)
    .bind(ip)
    .bind(ctx.request_id.as_deref())
    .execute(conn)
    .await?;
    Ok(())
}

/// Whole-row JSON snapshot of one record, used for before/after images.
/// `table` is always a literal from the calling code, never user input.
pub async fn snapshot(
    conn: &mut PgConnection,
    table: &str,
    id: Uuid,
) -> Result<Option<Value>, sqlx::Error> {
    let sql = format!("select to_jsonb(t) from {table} t where id = $1");
    sqlx::query_scalar(&sql).bind(id).fetch_optional(conn).await
}
