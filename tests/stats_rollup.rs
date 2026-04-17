use range_porter::stats::StatsRegistry;
use std::net::{Ipv4Addr, SocketAddr};

fn localhost(port: u16) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, port))
}

#[test]
fn rolls_minute_buckets_with_per_minute_deltas() {
    let stats = StatsRegistry::new(&[4_443], localhost(8_443), 4);
    let port_stats = stats.port(4_443);

    port_stats.record_tcp_accept();
    port_stats.add_tcp_bytes(10, 20);
    port_stats.record_udp_flow_open();
    port_stats.add_udp_in(30);
    port_stats.add_udp_out(40);
    stats.rollup_at_epoch(60);

    port_stats.record_tcp_close();
    port_stats.record_udp_flow_close();
    port_stats.add_tcp_bytes(5, 7);
    port_stats.add_udp_in(11);
    port_stats.add_udp_out(13);
    stats.rollup_at_epoch(120);

    let snapshot = stats.snapshot();
    assert_eq!(snapshot.minute_buckets.len(), 2);

    let first_bucket = &snapshot.minute_buckets[0];
    assert_eq!(first_bucket.minute_epoch, 1);
    assert_eq!(first_bucket.tcp_new_conn, 1);
    assert_eq!(first_bucket.tcp_active_peak, 1);
    assert_eq!(first_bucket.udp_active_flows_peak, 1);
    assert_eq!(first_bucket.tcp_in_bytes, 10);
    assert_eq!(first_bucket.tcp_out_bytes, 20);
    assert_eq!(first_bucket.udp_in_bytes, 30);
    assert_eq!(first_bucket.udp_out_bytes, 40);

    let second_bucket = &snapshot.minute_buckets[1];
    assert_eq!(second_bucket.minute_epoch, 2);
    assert_eq!(second_bucket.tcp_new_conn, 0);
    assert_eq!(second_bucket.tcp_active_peak, 0);
    assert_eq!(second_bucket.udp_active_flows_peak, 0);
    assert_eq!(second_bucket.tcp_in_bytes, 5);
    assert_eq!(second_bucket.tcp_out_bytes, 7);
    assert_eq!(second_bucket.udp_in_bytes, 11);
    assert_eq!(second_bucket.udp_out_bytes, 13);
}
