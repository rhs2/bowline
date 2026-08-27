//! Connection pool and migrations. The migration set is embedded at compile time
//! from `db/migrations` at the repository root.

use std::time::Duration;

use sqlx::migrate::Migrator;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

pub static MIGRATOR: Migrator = sqlx::migrate!("../db/migrations");

pub async fn connect(url: &str, max_connections: u32) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(10))
        .connect(url)
        .await
}

/// Applies pending migrations. A database whose schema was created by running the
/// SQL files directly (no `_sqlx_migrations` table yet) is baselined first: every
/// embedded migration is recorded as applied, with its checksum, so later files are
/// applied normally and checksum drift is still detected.
pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::Error> {
    let has_tracking: bool =
        sqlx::query_scalar("select to_regclass('public._sqlx_migrations') is not null")
            .fetch_one(pool)
            .await?;
    let has_schema: bool = sqlx::query_scalar("select to_regclass('public.employees') is not null")
        .fetch_one(pool)
        .await?;
    if !has_tracking && has_schema {
        tracing::warn!("schema exists without migration history; recording a baseline");
        baseline(pool).await?;
    }
    MIGRATOR.run(pool).await?;
    Ok(())
}

async fn baseline(pool: &PgPool) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "create table if not exists _sqlx_migrations (
            version bigint primary key,
            description text not null,
            installed_on timestamptz not null default now(),
            success boolean not null,
            checksum bytea not null,
            execution_time bigint not null)",
    )
    .execute(&mut *tx)
    .await?;
    for m in MIGRATOR.iter() {
        sqlx::query(
            "insert into _sqlx_migrations (version, description, success, checksum, execution_time)
             values ($1, $2, true, $3, 0) on conflict (version) do nothing",
        )
        .bind(m.version)
        .bind(m.description.as_ref())
        .bind(m.checksum.as_ref())
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await
}

/// Versions embedded in the binary that the database has not applied yet.
pub async fn pending_migrations(pool: &PgPool) -> Result<Vec<i64>, sqlx::Error> {
    let has_tracking: bool =
        sqlx::query_scalar("select to_regclass('public._sqlx_migrations') is not null")
            .fetch_one(pool)
            .await?;
    let applied: Vec<i64> = if has_tracking {
        sqlx::query_scalar("select version from _sqlx_migrations where success")
            .fetch_all(pool)
            .await?
    } else {
        Vec::new()
    };
    Ok(MIGRATOR
        .iter()
        .map(|m| m.version)
        .filter(|v| !applied.contains(v))
        .collect())
}
