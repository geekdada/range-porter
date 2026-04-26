mod common;

use common::{available_dual_port, localhost, start_app};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Duration, timeout};

#[tokio::test]
async fn forwards_tcp_streams_to_the_target() {
    let target_listener = TcpListener::bind(localhost(0))
        .await
        .expect("bind target tcp listener");
    let target_addr = target_listener.local_addr().expect("read target addr");

    let echo_task = tokio::spawn(async move {
        let (mut stream, _) = target_listener.accept().await.expect("accept target TCP");
        let mut buffer = [0_u8; 256];
        let bytes_read = stream.read(&mut buffer).await.expect("read target stream");
        stream
            .write_all(&buffer[..bytes_read])
            .await
            .expect("write target stream");
    });

    let listen_port = available_dual_port();
    let app = start_app(listen_port, target_addr).await;

    let mut client = TcpStream::connect(localhost(listen_port))
        .await
        .expect("connect to forwarder");
    client
        .write_all(b"hello over tcp")
        .await
        .expect("write client payload");
    client.shutdown().await.expect("shutdown client write half");

    let mut echoed = Vec::new();
    timeout(Duration::from_secs(2), client.read_to_end(&mut echoed))
        .await
        .expect("tcp read timed out")
        .expect("read echoed payload");

    assert_eq!(echoed, b"hello over tcp");
    echo_task.await.expect("tcp echo task");

    let snapshot = app.snapshot();
    assert_eq!(snapshot.totals.tcp_accepted, 1);
    assert_eq!(snapshot.totals.tcp_closed, 1);
    assert_eq!(snapshot.totals.tcp_in_bytes, 14);
    assert_eq!(snapshot.totals.tcp_out_bytes, 14);

    app.shutdown().await.expect("shutdown range-porter");
}

#[tokio::test]
async fn shutdown_completes_with_active_tcp_connection() {
    let target_listener = TcpListener::bind(localhost(0))
        .await
        .expect("bind target tcp listener");
    let target_addr = target_listener.local_addr().expect("read target addr");
    let (accepted_tx, accepted_rx) = tokio::sync::oneshot::channel();

    let target_task = tokio::spawn(async move {
        let (stream, _) = target_listener.accept().await.expect("accept target TCP");
        let _ = accepted_tx.send(());
        std::future::pending::<()>().await;
        drop(stream);
    });

    let listen_port = available_dual_port();
    let app = start_app(listen_port, target_addr).await;
    let _client = TcpStream::connect(localhost(listen_port))
        .await
        .expect("connect to forwarder");

    timeout(Duration::from_secs(2), accepted_rx)
        .await
        .expect("target accept timed out")
        .expect("target task exited before accepting");

    timeout(Duration::from_secs(2), app.shutdown())
        .await
        .expect("shutdown timed out")
        .expect("shutdown range-porter");

    target_task.abort();
    let _ = target_task.await;
}
