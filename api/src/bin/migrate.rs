//! Applies db/migrations to DATABASE_URL and exits.

use anyhow::Context;

use bowline_api::{db, telemetry, Config};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = Config::from_env().context("loading configuration")?;
    telemetry::init(config.log_format);
    let pool = db::connect(&config.database_url, 4)
        .await
        .context("connecting to postgres")?;
    let pending_before = db::pending_migrations(&pool).await?;
    db::migrate(&pool).await.context("applying migrations")?;
    let pending_after = db::pending_migrations(&pool).await?;
    anyhow::ensure!(
        pending_after.is_empty(),
        "migrations still pending after run: {pending_after:?}"
    );
    tracing::info!(
        applied = pending_before.len(),
        total = db::MIGRATOR.iter().count(),
        "migrations current"
    );
    Ok(())
}
