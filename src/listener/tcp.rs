use crate::forward;
use crate::stats::port::PortStats;
use crate::target::TargetAddr;
use anyhow::Result;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::warn;

pub async fn run(
    listener: TcpListener,
    target: Arc<TargetAddr>,
    stats: Arc<PortStats>,
    semaphore: Arc<Semaphore>,
    shutdown: CancellationToken,
) -> Result<()> {
    let mut connections = JoinSet::new();

    loop {
        // Acquire a slot before accepting; once the global cap is hit
        // the listener stops draining the kernel TCP backlog, providing
        // backpressure to clients without crashing the UDP path.
        let permit = tokio::select! {
            _ = shutdown.cancelled() => break,
            permit = Arc::clone(&semaphore).acquire_owned() => match permit {
                Ok(p) => p,
                Err(_) => break, // semaphore closed → shutdown path
            },
        };

        tokio::select! {
            _ = shutdown.cancelled() => break,
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, peer)) => {
                        stats.record_tcp_accept();

                        let stats = Arc::clone(&stats);
                        let target = Arc::clone(&target);
                        let shutdown = shutdown.child_token();
                        connections.spawn(async move {
                            let _permit = permit;
                            forward::tcp::proxy(stream, peer, target, stats, shutdown).await;
                        });
                    }
                    Err(error) => {
                        warn!(target = %target.display(), ?error, "tcp accept failed");
                        drop(permit);
                    }
                }
            }
        }
    }

    while let Some(join_result) = connections.join_next().await {
        if let Err(error) = join_result {
            warn!(?error, "tcp connection task panicked");
        }
    }

    Ok(())
}
