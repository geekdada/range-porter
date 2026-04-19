use anyhow::{Context, Result};
use clap::Parser;
use range_porter::cli::Cli;
use range_porter::config::RuntimeConfig;
use range_porter::runtime;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let cli = Cli::parse();
    let config = RuntimeConfig::from_cli(cli).await?;
    let startup_config = config.clone();

    let app = runtime::start(config).await?;
    info!(
        listen_host = %startup_config.listen_host,
        listen_ports = ?startup_config.listen_ports,
        target = startup_config.target.display(),
        target_addr = %startup_config.target.current(),
        stats_bind = app.stats_bind().map(|addr| addr.to_string()).unwrap_or_else(|| "disabled".to_string()),
        "range-porter started"
    );

    tokio::signal::ctrl_c()
        .await
        .context("failed to listen for shutdown signal")?;
    info!("shutdown signal received");

    app.shutdown().await
}

fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .compact()
        .init();
}
