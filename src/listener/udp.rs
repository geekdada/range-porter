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
    let mut buffer = vec![0_u8; 65_535];

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
            receive_result = socket.recv_from(&mut buffer) => {
                match receive_result {
                    Ok((bytes_read, source)) => {
                        if bytes_read == 0 {
                            continue;
                        }

                        match sessions.get_or_create(source) {
                            Ok(_) => {
                                if let Err(error) = sessions.forward_client_packet(source, &buffer[..bytes_read]).await {
                                    warn!(%source, %target, ?error, "failed to forward UDP datagram to target");
                                }
                            }
                            Err(error) => {
                                warn!(%source, %target, ?error, "failed to create UDP session");
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
