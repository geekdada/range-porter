use crate::stats::port::PortStats;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tracing::warn;

const COPY_BUFFER_BYTES: usize = 256 * 1024;

pub async fn proxy(
    mut downstream: TcpStream,
    peer: SocketAddr,
    target: SocketAddr,
    stats: Arc<PortStats>,
) {
    let _ = downstream.set_nodelay(true);

    let result = async {
        let mut upstream = TcpStream::connect(target).await?;
        let _ = upstream.set_nodelay(true);
        tokio::io::copy_bidirectional_with_sizes(
            &mut downstream,
            &mut upstream,
            COPY_BUFFER_BYTES,
            COPY_BUFFER_BYTES,
        )
        .await
    }
    .await;

    match result {
        Ok((in_bytes, out_bytes)) => {
            stats.add_tcp_bytes(in_bytes, out_bytes);
        }
        Err(error) => {
            warn!(%peer, %target, ?error, "tcp forwarding task ended with an error");
        }
    }

    stats.record_tcp_close();
}
