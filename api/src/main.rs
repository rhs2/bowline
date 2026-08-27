use std::net::SocketAddr;

use anyhow::Context;
use axum_prometheus::metrics_exporter_prometheus::{Matcher, PrometheusBuilder};
use axum_prometheus::utils::SECONDS_DURATION_BUCKETS;
use axum_prometheus::{PrometheusMetricLayer, AXUM_HTTP_REQUESTS_DURATION_SECONDS};
use tokio::signal;

use bowline_api::{db, http, telemetry, AppState, Config};

/// Stack for the thread that builds and runs the server.
///
/// The router is one deeply nested tower type per route and per layer, and with
/// ninety-odd routes an unoptimised build needs more stack to construct it than the
/// 8 MiB a main thread is given. A release build folds most of that away, but the
/// service must start under `cargo run` and in tests too, so the space is reserved
/// explicitly instead of being left to the optimiser.
const STACK_SIZE: usize = 32 * 1024 * 1024;

fn main() -> anyhow::Result<()> {
    std::thread::Builder::new()
        .name("bowline-main".to_string())
        .stack_size(STACK_SIZE)
        .spawn(run)
        .context("spawning the server thread")?
        .join()
        .map_err(|_| anyhow::anyhow!("the server thread panicked"))?
}

fn run() -> anyhow::Result<()> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(STACK_SIZE)
        .build()
        .context("building the tokio runtime")?
        .block_on(serve())
}

async fn serve() -> anyhow::Result<()> {
    let config = Config::from_env().context("loading configuration")?;
    telemetry::init(config.log_format);

    let pool = db::connect(&config.database_url, config.database_max_connections)
        .await
        .context("connecting to postgres")?;
    if config.database_migrate_on_start {
        db::migrate(&pool).await.context("applying migrations")?;
        tracing::info!("migrations current");
    } else {
        let pending = db::pending_migrations(&pool).await?;
        anyhow::ensure!(
            pending.is_empty(),
            "{} migration(s) pending and DATABASE_MIGRATE_ON_START=0: {:?}",
            pending.len(),
            pending
        );
    }

    // Install the recorder by hand rather than with `PrometheusMetricLayer::pair()`.
    // That helper builds a full exporter, which opens its own HTTP listener on port
    // 9000 and panics when anything else already holds it. This service publishes its
    // own /metrics from the handle, so the extra listener is unwanted as well as
    // fragile. `install_recorder` registers the global recorder and nothing else.
    let metrics_handle = PrometheusBuilder::new()
        .set_buckets_for_metric(
            Matcher::Full(AXUM_HTTP_REQUESTS_DURATION_SECONDS.to_string()),
            SECONDS_DURATION_BUCKETS,
        )
        .context("configuring latency buckets")?
        .install_recorder()
        .context("installing the metrics recorder")?;
    let metric_layer = PrometheusMetricLayer::new();
    let bind = config.api_bind;
    let state = AppState::build(config, pool, Some(metrics_handle)).await;
    let app = http::router(state, Some(metric_layer));

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("binding {bind}"))?;
    tracing::info!(%bind, "bowline-api listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .context("server error")?;
    tracing::info!("shutdown complete");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) = signal::unix::signal(signal::unix::SignalKind::terminate()) {
            sig.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}
