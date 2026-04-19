use range_porter::RuntimeConfig;
use range_porter::runtime::{self, RunningApp};
use range_porter::target::TargetAddr;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

pub fn localhost(port: u16) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, port))
}

pub fn available_dual_port() -> u16 {
    loop {
        let tcp = std::net::TcpListener::bind(localhost(0)).expect("bind ephemeral TCP port");
        let port = tcp.local_addr().expect("read TCP local addr").port();

        match std::net::UdpSocket::bind(localhost(port)) {
            Ok(udp) => {
                drop(udp);
                drop(tcp);
                return port;
            }
            Err(_) => continue,
        }
    }
}

pub async fn start_app(listen_port: u16, target: SocketAddr) -> RunningApp {
    let target_addr = TargetAddr::bind(&target.to_string(), None)
        .await
        .expect("bind target address");

    let config = RuntimeConfig::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        vec![listen_port],
        Arc::new(target_addr),
        Duration::from_secs(2),
        Some(localhost(0)),
        60,
        Duration::from_secs(0),
    )
    .expect("build runtime config");

    runtime::start(config).await.expect("start range-porter")
}
