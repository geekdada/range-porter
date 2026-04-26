use crate::listener::udp_batch::{self, BatchBuf, LISTENER_BATCH_SIZE, LISTENER_SLOT_SIZE};
use crate::listener::udp_send;
use crate::stats::port::PortStats;
use crate::target::TargetAddr;
use crate::udp_session::UdpSessionTable;
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio_util::sync::CancellationToken;
use tracing::warn;

pub async fn run(
    socket: UdpSocket,
    target: Arc<TargetAddr>,
    idle_timeout: Duration,
    stats: Arc<PortStats>,
    shutdown: CancellationToken,
) -> Result<()> {
    let socket = Arc::new(socket);
    let (reply_tx, reply_handle) = udp_send::spawn_reply_sender(
        Arc::clone(&socket),
        Arc::clone(&stats),
        shutdown.clone(),
    );
    let sessions = UdpSessionTable::new(
        Arc::clone(&target),
        idle_timeout,
        Arc::clone(&stats),
        shutdown.clone(),
        reply_tx,
    );
    let cleanup_task = tokio::spawn(Arc::clone(&sessions).cleanup_loop());
    let mut batch = BatchBuf::new(LISTENER_BATCH_SIZE, LISTENER_SLOT_SIZE);

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
                                        warn!(%source, target = %target.display(), ?error, "failed to forward UDP datagram to target");
                                    }
                                }
                                Err(error) => {
                                    warn!(%source, target = %target.display(), ?error, "failed to create UDP session");
                                }
                            }
                        }
                    }
                    Err(error) => {
                        warn!(target = %target.display(), ?error, "udp receive loop failed");
                    }
                }
            }
        }
    }

    sessions.shutdown_all();

    if let Err(error) = cleanup_task.await {
        warn!(?error, "udp cleanup task panicked");
    }

    // Drop the table (and its Sender) so the reply task drains and exits.
    drop(sessions);
    if let Err(error) = reply_handle.await {
        warn!(?error, "udp reply sender task panicked");
    }

    Ok(())
}
