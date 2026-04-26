use crate::config::RuntimeConfig;
use crate::http;
use crate::listener;
use crate::socket::{bind_tcp_listener, bind_udp_socket};
use crate::stats::StatsRegistry;
use crate::target::TargetAddr;
use anyhow::{Context, Result, anyhow};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::info;

pub struct RunningApp {
    stats_bind: Option<SocketAddr>,
    stats: Arc<StatsRegistry>,
    shutdown: CancellationToken,
    tasks: JoinSet<Result<()>>,
    tcp_semaphore: Arc<Semaphore>,
}

pub async fn start(config: RuntimeConfig) -> Result<RunningApp> {
    let target: Arc<TargetAddr> = config.target;
    let udp_idle_timeout = config.udp_idle_timeout;
    let tcp_semaphore = Arc::new(Semaphore::new(config.max_tcp_connections));

    let stats = Arc::new(StatsRegistry::new(
        &config.listen_ports,
        target.display().to_string(),
        config.stats_window,
    ));
    let shutdown = CancellationToken::new();

    let stats_endpoint = match config.stats_bind {
        Some(addr) => {
            let listener = bind_tcp_listener(addr)
                .with_context(|| format!("failed to bind stats listener on {addr}"))?;
            let bound = listener
                .local_addr()
                .context("failed to read bound stats address")?;
            Some((listener, bound))
        }
        None => None,
    };
    let stats_bind = stats_endpoint.as_ref().map(|(_, bound)| *bound);

    let mut listeners = Vec::with_capacity(config.listen_ports.len());
    for port in &config.listen_ports {
        let listen_addr = SocketAddr::new(config.listen_host, *port);
        let tcp_listener = bind_tcp_listener(listen_addr)
            .with_context(|| format!("failed to bind TCP listener on {listen_addr}"))?;
        let udp_socket = bind_udp_socket(listen_addr)
            .with_context(|| format!("failed to bind UDP socket on {listen_addr}"))?;
        listeners.push((*port, tcp_listener, udp_socket));
    }

    let mut tasks = JoinSet::new();

    {
        let stats = Arc::clone(&stats);
        let shutdown = shutdown.child_token();
        tasks.spawn(async move {
            stats.run_rollup(shutdown).await;
            Ok(())
        });
    }

    if !config.summary_interval.is_zero() {
        let stats = Arc::clone(&stats);
        let shutdown = shutdown.child_token();
        let interval = config.summary_interval;
        tasks.spawn(async move {
            summary_loop(stats, interval, shutdown).await;
            Ok(())
        });
    }

    if let Some((stats_listener, _)) = stats_endpoint {
        let stats = Arc::clone(&stats);
        let shutdown = shutdown.child_token();
        tasks.spawn(async move { http::serve(stats_listener, stats, shutdown).await });
    }

    for (port, tcp_listener, udp_socket) in listeners {
        let port_stats = stats.port(port);

        {
            let port_stats = Arc::clone(&port_stats);
            let shutdown = shutdown.child_token();
            let target = Arc::clone(&target);
            let semaphore = Arc::clone(&tcp_semaphore);
            tasks.spawn(async move {
                listener::tcp::run(tcp_listener, target, port_stats, semaphore, shutdown).await
            });
        }

        {
            let port_stats = Arc::clone(&port_stats);
            let shutdown = shutdown.child_token();
            let target = Arc::clone(&target);
            tasks.spawn(async move {
                listener::udp::run(udp_socket, target, udp_idle_timeout, port_stats, shutdown).await
            });
        }
    }

    Ok(RunningApp {
        stats_bind,
        stats,
        shutdown,
        tasks,
        tcp_semaphore,
    })
}

impl RunningApp {
    pub fn stats_bind(&self) -> Option<SocketAddr> {
        self.stats_bind
    }

    pub fn snapshot(&self) -> crate::stats::StatsSnapshot {
        self.stats.snapshot()
    }

    pub async fn shutdown(mut self) -> Result<()> {
        self.shutdown.cancel();
        // Wake any TCP listeners parked on `acquire_owned`.
        self.tcp_semaphore.close();
        let mut first_error = None;

        while let Some(join_result) = self.tasks.join_next().await {
            match join_result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(anyhow!("task join failure: {error}"));
                    }
                }
            }
        }

        if let Some(error) = first_error {
            return Err(error);
        }

        Ok(())
    }
}

async fn summary_loop(
    stats: Arc<StatsRegistry>,
    interval_duration: std::time::Duration,
    shutdown: CancellationToken,
) {
    let mut interval = tokio::time::interval(interval_duration);

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
            _ = interval.tick() => {
                let snapshot = stats.snapshot();
                info!(
                    tcp_active = snapshot.totals.tcp_active,
                    udp_active_flows = snapshot.totals.udp_active_flows,
                    tcp_in_bytes = snapshot.totals.tcp_in_bytes,
                    tcp_out_bytes = snapshot.totals.tcp_out_bytes,
                    udp_in_bytes = snapshot.totals.udp_in_bytes,
                    udp_out_bytes = snapshot.totals.udp_out_bytes,
                    "range-porter summary"
                );
            }
        }
    }
}
