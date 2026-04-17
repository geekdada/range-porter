mod common;

use common::{available_dual_port, localhost, start_app};
use tokio::net::UdpSocket;
use tokio::time::{Duration, sleep, timeout};

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
