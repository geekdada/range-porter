use crate::socket::configure_tcp_stream;
use crate::stats::port::PortStats;
use crate::target::TargetAddr;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;
use tracing::warn;

const COPY_BUFFER_BYTES: usize = 256 * 1024;

pub async fn proxy(
    mut downstream: TcpStream,
    peer: SocketAddr,
    target: Arc<TargetAddr>,
    stats: Arc<PortStats>,
    shutdown: CancellationToken,
) {
    let target_addr = target.current();
    if let Err(error) = configure_tcp_stream(&downstream) {
        warn!(%peer, ?error, "failed to set tcp options on downstream socket");
    }

    let forwarding = async {
        let mut upstream = TcpStream::connect(target_addr).await?;
        if let Err(error) = configure_tcp_stream(&upstream) {
            warn!(target = %target_addr, ?error, "failed to set tcp options on upstream socket");
        }
        tokio::io::copy_bidirectional_with_sizes(
            &mut downstream,
            &mut upstream,
            COPY_BUFFER_BYTES,
            COPY_BUFFER_BYTES,
        )
        .await
    };

    tokio::select! {
        _ = shutdown.cancelled() => {}
        result = forwarding => {
            match result {
                Ok((in_bytes, out_bytes)) => {
                    stats.add_tcp_bytes(in_bytes, out_bytes);
                }
                Err(error) => {
                    warn!(%peer, target = %target_addr, ?error, "tcp forwarding task ended with an error");
                }
            }
        }
    }

    stats.record_tcp_close();
}
