use crate::stats::{StatsRegistry, StatsSnapshot};
use anyhow::Result;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

pub async fn serve(
    listener: TcpListener,
    stats: Arc<StatsRegistry>,
    shutdown: CancellationToken,
) -> Result<()> {
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/stats", get(stats_snapshot))
        .with_state(stats);

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown.cancelled().await;
        })
        .await?;

    Ok(())
}

async fn healthz() -> &'static str {
    "ok"
}

async fn stats_snapshot(State(stats): State<Arc<StatsRegistry>>) -> Json<StatsSnapshot> {
    Json(stats.snapshot())
}
