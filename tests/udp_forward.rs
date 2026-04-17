mod common;

use common::{available_dual_port, localhost, start_app};
use socket2::{Domain, Protocol, Socket, Type};
use std::net::SocketAddr;
use tokio::net::UdpSocket;
use tokio::time::{Duration, sleep, timeout};

const LARGE_UDP_SOCKET_BUFFER_BYTES: usize = 65_535;

fn bind_tuned_udp_socket(address: SocketAddr) -> UdpSocket {
    let socket = Socket::new(domain_for(address), Type::DGRAM, Some(Protocol::UDP))
        .expect("create tuned UDP socket");
    socket
        .set_send_buffer_size(LARGE_UDP_SOCKET_BUFFER_BYTES)
        .expect("set tuned UDP send buffer");
    socket
        .set_recv_buffer_size(LARGE_UDP_SOCKET_BUFFER_BYTES)
        .expect("set tuned UDP recv buffer");
    socket.bind(&address.into()).expect("bind tuned UDP socket");
    socket
        .set_nonblocking(true)
        .expect("set tuned UDP socket nonblocking");

    let std_socket: std::net::UdpSocket = socket.into();
    UdpSocket::from_std(std_socket).expect("convert tuned UDP socket")
}

fn domain_for(address: SocketAddr) -> Domain {
    if address.is_ipv4() {
        Domain::IPV4
    } else {
        Domain::IPV6
    }
}

#[tokio::test]
async fn forwards_udp_datagrams_and_replies() {
    let target_socket = UdpSocket::bind(localhost(0))
        .await
        .expect("bind target udp socket");
    let target_addr = target_socket.local_addr().expect("read target UDP addr");

    let echo_task = tokio::spawn(async move {
        let mut buffer = [0_u8; 256];
        let (bytes_read, peer) = target_socket
            .recv_from(&mut buffer)
            .await
            .expect("recv target UDP");
        target_socket
            .send_to(&buffer[..bytes_read], peer)
            .await
            .expect("send target UDP");
    });

    let listen_port = available_dual_port();
    let app = start_app(listen_port, target_addr).await;

    let client = UdpSocket::bind(localhost(0))
        .await
        .expect("bind client udp socket");
    client
        .send_to(b"hello over udp", localhost(listen_port))
        .await
        .expect("send client datagram");

    let mut buffer = [0_u8; 256];
    let (bytes_read, peer) = timeout(Duration::from_secs(2), client.recv_from(&mut buffer))
        .await
        .expect("udp read timed out")
        .expect("recv echoed datagram");

    assert_eq!(&buffer[..bytes_read], b"hello over udp");
    assert_eq!(peer, localhost(listen_port));
    echo_task.await.expect("udp echo task");

    sleep(Duration::from_millis(50)).await;

    let snapshot = app.snapshot();
    assert_eq!(snapshot.totals.udp_in_bytes, 14);
    assert_eq!(snapshot.totals.udp_out_bytes, 14);
    assert_eq!(snapshot.totals.udp_active_flows, 1);

    app.shutdown().await.expect("shutdown range-porter");
}

#[tokio::test]
async fn forwards_large_udp_datagrams_and_replies() {
    let payload = vec![0x5a; 16 * 1024];
    // Tune the test endpoints so the forwarder's own socket settings determine success.
    let target_socket = bind_tuned_udp_socket(localhost(0));
    let target_addr = target_socket.local_addr().expect("read target UDP addr");

    let expected_payload = payload.clone();
    let echo_task = tokio::spawn(async move {
        let mut buffer = vec![0_u8; expected_payload.len()];
        let (bytes_read, peer) = target_socket
            .recv_from(&mut buffer)
            .await
            .expect("recv target UDP");
        assert_eq!(&buffer[..bytes_read], expected_payload.as_slice());
        target_socket
            .send_to(&buffer[..bytes_read], peer)
            .await
            .expect("send target UDP");
    });

    let listen_port = available_dual_port();
    let app = start_app(listen_port, target_addr).await;

    let client = bind_tuned_udp_socket(localhost(0));
    client
        .send_to(&payload, localhost(listen_port))
        .await
        .expect("send large client datagram");

    let mut buffer = vec![0_u8; payload.len()];
    let (bytes_read, peer) = timeout(Duration::from_secs(2), client.recv_from(&mut buffer))
        .await
        .expect("large udp read timed out")
        .expect("recv echoed large datagram");

    assert_eq!(&buffer[..bytes_read], payload.as_slice());
    assert_eq!(peer, localhost(listen_port));
    echo_task.await.expect("large udp echo task");

    sleep(Duration::from_millis(50)).await;

    let snapshot = app.snapshot();
    assert_eq!(snapshot.totals.udp_in_bytes, payload.len() as u64);
    assert_eq!(snapshot.totals.udp_out_bytes, payload.len() as u64);
    assert_eq!(snapshot.totals.udp_active_flows, 1);

    app.shutdown().await.expect("shutdown range-porter");
}
