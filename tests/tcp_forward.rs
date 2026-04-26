mod common;

use common::{available_dual_port, localhost, start_app, start_app_with_tcp_cap};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Duration, sleep, timeout};

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

#[tokio::test]
async fn tcp_connection_cap_blocks_excess_accepts() {
    // Spawn an upstream that accepts and parks every connection so each
    // forwarder permit stays held for the full duration of the test.
    let target_listener = TcpListener::bind(localhost(0))
        .await
        .expect("bind target tcp listener");
    let target_addr = target_listener.local_addr().expect("read target addr");

    let target_task = tokio::spawn(async move {
        while let Ok((mut stream, _)) = target_listener.accept().await {
            tokio::spawn(async move {
                // Drain the connection until the client closes its write
                // half; that lets the forwarder's bidirectional copy
                // terminate and release the cap permit.
                let mut buf = [0u8; 1024];
                loop {
                    match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                }
            });
        }
    });

    let listen_port = available_dual_port();
    let app = start_app_with_tcp_cap(listen_port, target_addr, 2).await;

    // Wait for forwarder to fully bind before connecting.
    sleep(Duration::from_millis(50)).await;

    let mut c1 = TcpStream::connect(localhost(listen_port))
        .await
        .expect("connect c1");
    let c2 = TcpStream::connect(localhost(listen_port))
        .await
        .expect("connect c2");

    // Allow the forwarder time to record both accepts.
    for _ in 0..50 {
        if app.snapshot().totals.tcp_active == 2 {
            break;
        }
        sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(app.snapshot().totals.tcp_active, 2);

    // Third connection: kernel SYN/ACK still completes but the forwarder
    // will not call accept() until a permit frees up. Verify tcp_active
    // stays at 2 for a meaningful window.
    let _c3_task = tokio::spawn(async move {
        let _ = TcpStream::connect(localhost(listen_port)).await;
    });
    sleep(Duration::from_millis(200)).await;
    assert_eq!(
        app.snapshot().totals.tcp_active,
        2,
        "third connection should not have been accepted while cap is full"
    );

    // Free a slot by closing c1; the third connection should be accepted.
    c1.shutdown().await.expect("shutdown c1");
    drop(c1);
    for _ in 0..100 {
        let active = app.snapshot().totals.tcp_active;
        if active >= 2 {
            // c2 still active + c3 newly accepted ⇒ active back to 2.
            // We just need to confirm tcp_accepted advanced beyond 2.
            if app.snapshot().totals.tcp_accepted >= 3 {
                break;
            }
        }
        sleep(Duration::from_millis(20)).await;
    }
    assert!(
        app.snapshot().totals.tcp_accepted >= 3,
        "third connection should have been accepted after cap freed; got {}",
        app.snapshot().totals.tcp_accepted
    );

    let mut c2 = c2;
    c2.shutdown().await.ok();
    drop(c2);
    timeout(Duration::from_secs(2), app.shutdown())
        .await
        .expect("shutdown timed out")
        .expect("shutdown range-porter");

    target_task.abort();
    let _ = target_task.await;
}
