use crate::forward;
use crate::stats::port::PortStats;
use crate::target::TargetAddr;
use anyhow::Result;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::warn;

pub async fn run(
    listener: TcpListener,
    target: Arc<TargetAddr>,
    stats: Arc<PortStats>,
    shutdown: CancellationToken,
) -> Result<()> {
    let mut connections = JoinSet::new();

    loop {
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
                            forward::tcp::proxy(stream, peer, target, stats, shutdown).await;
                        });
                    }
                    Err(error) => {
                        warn!(target = %target.display(), ?error, "tcp accept failed");
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
