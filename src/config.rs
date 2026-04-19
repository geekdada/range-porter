use crate::cli::Cli;
use crate::portset::parse_portset;
use crate::target::TargetAddr;
use anyhow::{Result, bail};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub listen_host: IpAddr,
    pub listen_ports: Vec<u16>,
    pub target: Arc<TargetAddr>,
    pub udp_idle_timeout: Duration,
    pub stats_bind: Option<SocketAddr>,
    pub stats_window: usize,
    pub summary_interval: Duration,
}

impl RuntimeConfig {
    pub fn new(
        listen_host: IpAddr,
        listen_ports: Vec<u16>,
        target: Arc<TargetAddr>,
        udp_idle_timeout: Duration,
        stats_bind: Option<SocketAddr>,
        stats_window: usize,
        summary_interval: Duration,
    ) -> Result<Self> {
        if listen_ports.is_empty() {
            bail!("at least one listen port is required");
        }

        if udp_idle_timeout.is_zero() {
            bail!("udp idle timeout must be greater than 0");
        }

        if stats_window == 0 {
            bail!("stats window must be greater than 0");
        }

        Ok(Self {
            listen_host,
            listen_ports,
            target,
            udp_idle_timeout,
            stats_bind,
            stats_window,
            summary_interval,
        })
    }

    pub async fn from_cli(cli: Cli) -> Result<Self> {
        let listen_ports = parse_portset(&cli.listen_ports)?;
        let target = Arc::new(TargetAddr::bind(&cli.target, cli.dns_server).await?);
        Self::new(
            cli.listen_host,
            listen_ports,
            target,
            cli.udp_idle_timeout,
            cli.stats_bind,
            cli.stats_window,
            cli.summary_interval,
        )
    }
}
