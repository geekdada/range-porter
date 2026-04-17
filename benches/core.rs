use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use range_porter::portset::parse_portset;
use range_porter::stats::StatsRegistry;
use range_porter::stats::port::PortStats;
use std::hint::black_box;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

struct StatsFixture {
    registry: StatsRegistry,
    port_handles: Vec<Arc<PortStats>>,
}

fn localhost(port: u16) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, port))
}

fn build_ports(port_count: usize) -> Vec<u16> {
    (0..port_count)
        .map(|offset| 20_000 + offset as u16)
        .collect()
}

fn seeded_stats_fixture(port_count: usize) -> StatsFixture {
    let ports = build_ports(port_count);
    let registry = StatsRegistry::new(&ports, localhost(8_443), 8);
    let port_handles: Vec<_> = ports
        .iter()
        .copied()
        .map(|port| registry.port(port))
        .collect();

    for (index, port_stats) in port_handles.iter().enumerate() {
        port_stats.record_tcp_accept();
        port_stats.add_tcp_bytes(256 + index as u64, 512 + index as u64);
        port_stats.record_udp_flow_open();
        port_stats.add_udp_in(128 + index as u64);
        port_stats.add_udp_out(192 + index as u64);

        if index % 3 == 0 {
            port_stats.record_tcp_close();
        }

        if index % 5 == 0 {
            port_stats.record_udp_flow_close();
        }
    }

    registry.rollup_at_epoch(60);

    StatsFixture {
        registry,
        port_handles,
    }
}

fn benchmark_parse_portset(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_portset");
    let mixed_expression = "80, 443, 10000-10032, 15000-15032, 443, 16000";
    let range_1024_ports = format!("20000-{}", 20_000 + 1_024 - 1);

    group.bench_function("mixed_expression", |b| {
        b.iter(|| {
            black_box(parse_portset(black_box(mixed_expression)).expect("mixed expression parses"))
        });
    });
    group.bench_function("range_1024_ports", |b| {
        b.iter(|| {
            black_box(
                parse_portset(black_box(range_1024_ports.as_str()))
                    .expect("range expression parses"),
            )
        });
    });
    group.finish();
}

fn benchmark_port_stats_updates(c: &mut Criterion) {
    let mut group = c.benchmark_group("port_stats_hot_path");

    group.bench_function("accept_close_and_bytes", |b| {
        let port_stats = PortStats::new(20_000);

        b.iter(|| {
            port_stats.record_tcp_accept();
            port_stats.add_tcp_bytes(black_box(1_024), black_box(2_048));
            port_stats.record_udp_flow_open();
            port_stats.add_udp_in(black_box(512));
            port_stats.add_udp_out(black_box(768));
            port_stats.record_tcp_close();
            port_stats.record_udp_flow_close();
        });
    });

    group.finish();
}

fn benchmark_stats_snapshot(c: &mut Criterion) {
    let mut group = c.benchmark_group("stats_snapshot");

    for port_count in [16usize, 256, 2_048] {
        group.bench_with_input(
            BenchmarkId::new("ports", port_count),
            &port_count,
            |b, &port_count| {
                let fixture = seeded_stats_fixture(port_count);
                b.iter(|| black_box(fixture.registry.snapshot()));
            },
        );
    }

    group.finish();
}

fn benchmark_stats_rollup(c: &mut Criterion) {
    let mut group = c.benchmark_group("stats_rollup");

    for port_count in [16usize, 256, 2_048] {
        group.bench_with_input(
            BenchmarkId::new("ports", port_count),
            &port_count,
            |b, &port_count| {
                let fixture = seeded_stats_fixture(port_count);
                let mut epoch_seconds = 120_u64;
                let mut port_index = 0usize;

                b.iter(|| {
                    let port_stats = &fixture.port_handles[port_index % fixture.port_handles.len()];
                    port_stats.record_tcp_accept();
                    port_stats.add_tcp_bytes(256, 512);
                    port_stats.record_udp_flow_open();
                    port_stats.add_udp_in(128);
                    port_stats.add_udp_out(256);
                    fixture.registry.rollup_at_epoch(black_box(epoch_seconds));
                    port_stats.record_tcp_close();
                    port_stats.record_udp_flow_close();

                    epoch_seconds += 60;
                    port_index += 1;
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    benchmark_parse_portset,
    benchmark_port_stats_updates,
    benchmark_stats_snapshot,
    benchmark_stats_rollup,
);
criterion_main!(benches);
