pub mod bucket;
pub mod port;

use crate::stats::bucket::{AggregateTotals, BucketRing, MinuteBucket};
use crate::stats::port::{PortSnapshot, PortStats};
use serde::Serialize;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
pub struct StatsRegistry {
    target: SocketAddr,
    started_at_epoch: u64,
    window_minutes: usize,
    ports: BTreeMap<u16, Arc<PortStats>>,
    buckets: Mutex<BucketRing>,
    previous_totals: Mutex<AggregateTotals>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatsSnapshot {
    pub target: SocketAddr,
    pub started_at_epoch: u64,
    pub generated_at_epoch: u64,
    pub window_minutes: usize,
    pub totals: AggregateTotals,
    pub ports: Vec<PortSnapshot>,
    pub minute_buckets: Vec<MinuteBucket>,
}

impl StatsRegistry {
    pub fn new(listen_ports: &[u16], target: SocketAddr, window_minutes: usize) -> Self {
        let ports = listen_ports
            .iter()
            .copied()
            .map(|port| (port, Arc::new(PortStats::new(port))))
            .collect();

        Self {
            target,
            started_at_epoch: unix_timestamp_now(),
            window_minutes,
            ports,
            buckets: Mutex::new(BucketRing::new(window_minutes)),
            previous_totals: Mutex::new(AggregateTotals::default()),
        }
    }

    pub fn port(&self, port: u16) -> Arc<PortStats> {
        self.ports
            .get(&port)
            .cloned()
            .unwrap_or_else(|| panic!("unknown port stats requested for port {port}"))
    }

    pub fn snapshot(&self) -> StatsSnapshot {
        let port_snapshots: Vec<_> = self.ports.values().map(|port| port.snapshot()).collect();
        let totals = aggregate_from_snapshots(&port_snapshots);
        let minute_buckets = self
            .buckets
            .lock()
            .expect("minute bucket lock poisoned")
            .snapshot();

        StatsSnapshot {
            target: self.target,
            started_at_epoch: self.started_at_epoch,
            generated_at_epoch: unix_timestamp_now(),
            window_minutes: self.window_minutes,
            totals,
            ports: port_snapshots,
            minute_buckets,
        }
    }

    pub fn rollup_now(&self) {
        self.rollup_at_epoch(unix_timestamp_now());
    }

    pub fn rollup_at_epoch(&self, now_epoch_seconds: u64) {
        let totals = aggregate_from_snapshots(
            &self
                .ports
                .values()
                .map(|port| port.snapshot())
                .collect::<Vec<_>>(),
        );

        let mut buckets = self.buckets.lock().expect("minute bucket lock poisoned");
        let mut previous_totals = self
            .previous_totals
            .lock()
            .expect("previous totals lock poisoned");
        buckets.rollup(now_epoch_seconds, &totals, &mut previous_totals);
    }

    pub async fn run_rollup(self: Arc<Self>, shutdown: CancellationToken) {
        let mut interval = tokio::time::interval(Duration::from_secs(1));

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = interval.tick() => self.rollup_now(),
            }
        }

        self.rollup_now();
    }
}

fn aggregate_from_snapshots(port_snapshots: &[PortSnapshot]) -> AggregateTotals {
    let mut totals = AggregateTotals::default();

    for snapshot in port_snapshots {
        totals.tcp_accepted += snapshot.tcp_accepted;
        totals.tcp_active += snapshot.tcp_active;
        totals.tcp_closed += snapshot.tcp_closed;
        totals.udp_active_flows += snapshot.udp_active_flows;
        totals.tcp_in_bytes += snapshot.tcp_in_bytes;
        totals.tcp_out_bytes += snapshot.tcp_out_bytes;
        totals.udp_in_bytes += snapshot.udp_in_bytes;
        totals.udp_out_bytes += snapshot.udp_out_bytes;
    }

    totals
}

fn unix_timestamp_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock drifted before unix epoch")
        .as_secs()
}
