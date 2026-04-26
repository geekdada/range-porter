use crate::listener::udp_batch::{self, ReplyPacket};
use crate::stats::port::PortStats;
use std::io;
use std::sync::Arc;
use tokio::io::Interest;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::warn;

/// Maximum reply queue depth before producers drop datagrams. UDP has no
/// delivery guarantee, so dropping under burst is preferable to unbounded
/// memory growth or back-pressuring the recv loop.
const REPLY_QUEUE_CAPACITY: usize = 512;

/// Maximum datagrams flushed in one `sendmmsg` syscall (matches the recv-side
/// listener batch ceiling).
const SEND_BATCH_SIZE: usize = 32;

/// Spawn a single batched sender task for the listener socket. Returns the
/// `Sender` to be cloned into per-session reply loops, and a `JoinHandle`
/// the caller awaits during shutdown.
pub fn spawn_reply_sender(
    listener: Arc<UdpSocket>,
    stats: Arc<PortStats>,
    shutdown: CancellationToken,
) -> (mpsc::Sender<ReplyPacket>, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel::<ReplyPacket>(REPLY_QUEUE_CAPACITY);
    let handle = tokio::spawn(reply_loop(listener, stats, rx, shutdown));
    (tx, handle)
}

async fn reply_loop(
    listener: Arc<UdpSocket>,
    stats: Arc<PortStats>,
    mut rx: mpsc::Receiver<ReplyPacket>,
    shutdown: CancellationToken,
) {
    let mut batch: Vec<ReplyPacket> = Vec::with_capacity(SEND_BATCH_SIZE);

    loop {
        batch.clear();

        let received = tokio::select! {
            _ = shutdown.cancelled() => break,
            n = rx.recv_many(&mut batch, SEND_BATCH_SIZE) => n,
        };

        if received == 0 {
            // All producers dropped → exit cleanly.
            break;
        }

        flush(&listener, &stats, &mut batch).await;
    }
}

async fn flush(listener: &UdpSocket, stats: &PortStats, batch: &mut [ReplyPacket]) {
    let mut start = 0;
    while start < batch.len() {
        match listener
            .async_io(Interest::WRITABLE, || {
                udp_batch::send_batch(listener, &batch[start..])
            })
            .await
        {
            Ok(0) => break,
            Ok(sent) => {
                for pkt in &batch[start..start + sent] {
                    stats.add_udp_out(pkt.bytes as u64);
                }
                start += sent;
            }
            Err(error) => match error.kind() {
                io::ErrorKind::Interrupted => continue,
                _ => {
                    warn!(?error, "udp reply batch send failed");
                    break;
                }
            },
        }
    }
}
