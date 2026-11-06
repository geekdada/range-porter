use socket2::{Domain, Protocol, SockRef, Socket, TcpKeepalive, Type};
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream, UdpSocket};

// 4 MiB fits under macOS kern.ipc.maxsockbuf (8 MiB) and raises Linux
// SO_RCVBUF/SO_SNDBUF up to net.core.rmem_max/wmem_max (silently capped
// if the sysctl is lower — admins can raise it for heavy workloads).
const UDP_SOCKET_BUFFER_BYTES: usize = 4 * 1024 * 1024;

// A peer that vanishes without FIN or RST is declared dead after about
// 60s + 6 * 10s of silence. The relay terminates TCP, so the far end's own
// keepalive only ever reaches us and cannot detect it.
const TCP_KEEPALIVE: TcpKeepalive = TcpKeepalive::new()
    .with_time(Duration::from_secs(60))
    .with_interval(Duration::from_secs(10))
    .with_retries(6);

pub fn configure_tcp_stream(stream: &TcpStream) -> io::Result<()> {
    stream.set_nodelay(true)?;
    SockRef::from(stream).set_tcp_keepalive(&TCP_KEEPALIVE)
}

pub fn bind_tcp_listener(address: SocketAddr) -> io::Result<TcpListener> {
    let socket = Socket::new(domain_for(&address), Type::STREAM, Some(Protocol::TCP))?;
    socket.set_reuse_address(true)?;
    socket.bind(&address.into())?;
    socket.listen(1024)?;
    socket.set_nonblocking(true)?;

    let std_listener: std::net::TcpListener = socket.into();
    TcpListener::from_std(std_listener)
}

pub fn bind_udp_socket(address: SocketAddr) -> io::Result<UdpSocket> {
    let socket = Socket::new(domain_for(&address), Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    configure_udp_socket(&socket)?;
    socket.bind(&address.into())?;
    socket.set_nonblocking(true)?;

    let std_socket: std::net::UdpSocket = socket.into();
    UdpSocket::from_std(std_socket)
}

pub fn new_connected_udp_socket(target: SocketAddr) -> io::Result<UdpSocket> {
    let bind_addr = match target {
        SocketAddr::V4(_) => SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)),
        SocketAddr::V6(_) => SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0)),
    };

    let socket = Socket::new(domain_for(&target), Type::DGRAM, Some(Protocol::UDP))?;
    configure_udp_socket(&socket)?;
    socket.bind(&bind_addr.into())?;
    socket.connect(&target.into())?;
    socket.set_nonblocking(true)?;

    let std_socket: std::net::UdpSocket = socket.into();
    UdpSocket::from_std(std_socket)
}

fn configure_udp_socket(socket: &Socket) -> io::Result<()> {
    // macOS defaults UDP send buffers to 9216 bytes, which turns larger
    // forwarded datagrams into EMSGSIZE even on loopback.
    socket.set_send_buffer_size(UDP_SOCKET_BUFFER_BYTES)?;
    socket.set_recv_buffer_size(UDP_SOCKET_BUFFER_BYTES)?;
    Ok(())
}

fn domain_for(address: &SocketAddr) -> Domain {
    if address.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn connected_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("bind loopback listener");
        let address = listener.local_addr().expect("read listener addr");
        let (client, accepted) = tokio::join!(TcpStream::connect(address), listener.accept());
        (
            client.expect("connect loopback"),
            accepted.expect("accept loopback").0,
        )
    }

    #[tokio::test]
    async fn configured_tcp_stream_probes_silent_peers() {
        let (stream, _peer) = connected_pair().await;

        configure_tcp_stream(&stream).expect("configure tcp stream");

        let socket = SockRef::from(&stream);
        assert!(socket.keepalive().expect("read SO_KEEPALIVE"));
        assert_eq!(
            socket.tcp_keepalive_time().expect("read keepalive time"),
            Duration::from_secs(60)
        );
        assert_eq!(
            socket
                .tcp_keepalive_interval()
                .expect("read keepalive interval"),
            Duration::from_secs(10)
        );
        assert_eq!(
            socket
                .tcp_keepalive_retries()
                .expect("read keepalive retries"),
            6
        );
        assert!(stream.nodelay().expect("read TCP_NODELAY"));
    }
}
