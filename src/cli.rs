use clap::Parser;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

#[derive(Debug, Clone, Parser)]
#[command(
    author,
    version,
    about = "High-efficiency TCP/UDP port-range forwarder"
)]
pub struct Cli {
    #[arg(long, default_value = "0.0.0.0")]
    pub listen_host: IpAddr,

    #[arg(
        long,
        value_name = "PORTS",
        help = "Port expression like 80,443,10000-10100"
    )]
    pub listen_ports: String,

    #[arg(long, value_name = "HOST:PORT", help = "Single TCP/UDP target address")]
    pub target: SocketAddr,

    #[arg(
        long,
        default_value = "60s",
        value_parser = parse_duration,
        help = "Idle timeout for UDP session state"
    )]
    pub udp_idle_timeout: Duration,

    #[arg(
        long,
        default_value = "127.0.0.1:9090",
        value_name = "HOST:PORT",
        help = "Bind address for the read-only JSON stats endpoint"
    )]
    pub stats_bind: SocketAddr,

    #[arg(
        long,
        default_value_t = 60,
        help = "Number of minute buckets to retain"
    )]
    pub stats_window: usize,

    #[arg(
        long,
        default_value = "60s",
        value_parser = parse_duration,
        help = "Periodic stdout/log summary interval; set to 0s to disable"
    )]
    pub summary_interval: Duration,
}

pub fn parse_duration(input: &str) -> Result<Duration, String> {
    humantime::parse_duration(input).map_err(|error| error.to_string())
}
