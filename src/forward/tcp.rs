use crate::stats::port::PortStats;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tracing::warn;

pub async fn proxy(
    mut downstream: TcpStream,
    peer: SocketAddr,
    target: SocketAddr,
    stats: Arc<PortStats>,
) {
    let result = async {
        let mut upstream = TcpStream::connect(target).await?;
        tokio::io::copy_bidirectional(&mut downstream, &mut upstream).await
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
