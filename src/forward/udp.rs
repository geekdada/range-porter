use std::io;
use tokio::net::UdpSocket;

pub async fn forward_to_target(upstream: &UdpSocket, payload: &[u8]) -> io::Result<usize> {
    upstream.send(payload).await
}
