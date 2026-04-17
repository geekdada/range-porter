use std::io;
use std::net::SocketAddr;
use tokio::net::UdpSocket;

pub async fn forward_to_target(upstream: &UdpSocket, payload: &[u8]) -> io::Result<usize> {
    upstream.send(payload).await
}

pub async fn forward_to_client(
    listener: &UdpSocket,
    source: SocketAddr,
    payload: &[u8],
) -> io::Result<usize> {
    listener.send_to(payload, source).await
}
