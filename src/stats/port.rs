use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug)]
pub struct PortStats {
    port: u16,
    tcp_accepted: AtomicU64,
    tcp_active: AtomicU64,
    tcp_closed: AtomicU64,
    udp_active_flows: AtomicU64,
    tcp_in_bytes: AtomicU64,
    tcp_out_bytes: AtomicU64,
    udp_in_bytes: AtomicU64,
    udp_out_bytes: AtomicU64,
    udp_dropped: AtomicU64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PortSnapshot {
    pub port: u16,
    pub tcp_accepted: u64,
    pub tcp_active: u64,
    pub tcp_closed: u64,
    pub udp_active_flows: u64,
    pub tcp_in_bytes: u64,
    pub tcp_out_bytes: u64,
    pub udp_in_bytes: u64,
    pub udp_out_bytes: u64,
    pub udp_dropped: u64,
}

impl PortStats {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            tcp_accepted: AtomicU64::new(0),
            tcp_active: AtomicU64::new(0),
            tcp_closed: AtomicU64::new(0),
            udp_active_flows: AtomicU64::new(0),
            tcp_in_bytes: AtomicU64::new(0),
            tcp_out_bytes: AtomicU64::new(0),
            udp_in_bytes: AtomicU64::new(0),
            udp_out_bytes: AtomicU64::new(0),
            udp_dropped: AtomicU64::new(0),
        }
    }

    pub fn record_tcp_accept(&self) {
        self.tcp_accepted.fetch_add(1, Ordering::Relaxed);
        self.tcp_active.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_tcp_close(&self) {
        atomic_saturating_decrement(&self.tcp_active);
        self.tcp_closed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_tcp_bytes(&self, in_bytes: u64, out_bytes: u64) {
        self.tcp_in_bytes.fetch_add(in_bytes, Ordering::Relaxed);
        self.tcp_out_bytes.fetch_add(out_bytes, Ordering::Relaxed);
    }

    pub fn record_udp_flow_open(&self) {
        self.udp_active_flows.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_udp_flow_close(&self) {
        atomic_saturating_decrement(&self.udp_active_flows);
    }

    pub fn add_udp_in(&self, bytes: u64) {
        self.udp_in_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn add_udp_out(&self, bytes: u64) {
        self.udp_out_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn record_udp_drop(&self) {
        self.udp_dropped.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> PortSnapshot {
        PortSnapshot {
            port: self.port,
            tcp_accepted: self.tcp_accepted.load(Ordering::Relaxed),
            tcp_active: self.tcp_active.load(Ordering::Relaxed),
            tcp_closed: self.tcp_closed.load(Ordering::Relaxed),
            udp_active_flows: self.udp_active_flows.load(Ordering::Relaxed),
            tcp_in_bytes: self.tcp_in_bytes.load(Ordering::Relaxed),
            tcp_out_bytes: self.tcp_out_bytes.load(Ordering::Relaxed),
            udp_in_bytes: self.udp_in_bytes.load(Ordering::Relaxed),
            udp_out_bytes: self.udp_out_bytes.load(Ordering::Relaxed),
            udp_dropped: self.udp_dropped.load(Ordering::Relaxed),
        }
    }
}

fn atomic_saturating_decrement(counter: &AtomicU64) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
        Some(current.saturating_sub(1))
    });
}
