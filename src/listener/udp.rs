use crate::listener::udp_batch::{self, BatchBuf};
use crate::stats::port::PortStats;
use crate::udp_session::UdpSessionTable;
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio_util::sync::CancellationToken;
use tracing::warn;

pub async fn run(
    socket: UdpSocket,
    target: SocketAddr,
    idle_timeout: Duration,
    stats: Arc<PortStats>,
    shutdown: CancellationToken,
) -> Result<()> {
    let socket = Arc::new(socket);
    let sessions = UdpSessionTable::new(
        target,
        idle_timeout,
        Arc::clone(&socket),
        Arc::clone(&stats),
        shutdown.clone(),
    );
    let cleanup_task = tokio::spawn(Arc::clone(&sessions).cleanup_loop());
    let mut batch = BatchBuf::new();

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
            result = udp_batch::recv_batch(&socket, &mut batch) => {
                match result {
                    Ok(count) => {
                        for i in 0..count {
                            let Some(source) = batch.addr(i) else { continue };
                            let payload = batch.slot(i);
                            if payload.is_empty() {
                                continue;
                            }

                            match sessions.get_or_create(source) {
                                Ok(session) => {
                                    if let Err(error) = sessions
                                        .forward_client_packet(&session, payload)
                                        .await
                                    {
                                        warn!(%source, %target, ?error, "failed to forward UDP datagram to target");
                                    }
                                }
                                Err(error) => {
                                    warn!(%source, %target, ?error, "failed to create UDP session");
                                }
                            }
                        }
                    }
                    Err(error) => {
                        warn!(%target, ?error, "udp receive loop failed");
                    }
                }
            }
        }
    }

    sessions.shutdown_all();

    if let Err(error) = cleanup_task.await {
        warn!(?error, "udp cleanup task panicked");
    }

    Ok(())
}
