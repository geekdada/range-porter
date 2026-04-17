use socket2::{Domain, Protocol, Socket, Type};
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use tokio::net::{TcpListener, UdpSocket};

// 4 MiB fits under macOS kern.ipc.maxsockbuf (8 MiB) and raises Linux
// SO_RCVBUF/SO_SNDBUF up to net.core.rmem_max/wmem_max (silently capped
// if the sysctl is lower — admins can raise it for heavy workloads).
const UDP_SOCKET_BUFFER_BYTES: usize = 4 * 1024 * 1024;

pub fn bind_tcp_listener(address: SocketAddr) -> io::Result<TcpListener> {
    let socket = Socket::new(domain_for(address), Type::STREAM, Some(Protocol::TCP))?;
    socket.set_reuse_address(true)?;
    socket.bind(&address.into())?;
    socket.listen(1024)?;
    socket.set_nonblocking(true)?;

    let std_listener: std::net::TcpListener = socket.into();
    TcpListener::from_std(std_listener)
}

pub fn bind_udp_socket(address: SocketAddr) -> io::Result<UdpSocket> {
    let socket = Socket::new(domain_for(address), Type::DGRAM, Some(Protocol::UDP))?;
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

    let socket = Socket::new(domain_for(target), Type::DGRAM, Some(Protocol::UDP))?;
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

fn domain_for(address: SocketAddr) -> Domain {
    if address.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    }
}
