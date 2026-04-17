# range-porter

`range-porter` is a single-binary Rust forwarder for operators who need one process to listen on a port set like `80,443,20000-50000` and forward both TCP and UDP traffic to a single target address.

It is built for server-side deployment, with a deliberately small feature set:

- shared TCP + UDP listen ports
- one target address per process
- lightweight in-memory minute buckets
- read-only JSON stats over HTTP

It does **not** do encryption, authentication, persistent storage, hot reload, or multi-target routing.

## Why it exists

This project is meant to be a pragmatic userspace forwarder. It is useful when you want:

- a generic port-range forwarder, not a Hysteria-specific wrapper
- simple traffic and connection visibility
- a single Rust binary you can audit and run under `systemd`

It is **not** a replacement for kernel NAT or `iptables`/`nftables` performance. Packets are still processed in userspace.

## Usage

```bash
cargo run -- \
  --listen-host 0.0.0.0 \
  --listen-ports 20000-50000 \
  --target 127.0.0.1:443 \
  --udp-idle-timeout 60s \
  --stats-bind 127.0.0.1:9090
```

Flags:

- `--listen-host`: host/IP to bind on
- `--listen-ports`: comma-separated ports and ranges
- `--target`: shared TCP/UDP target address
- `--udp-idle-timeout`: inactivity timeout for UDP session entries, default `60s`
- `--stats-bind`: bind address for the JSON stats endpoint
- `--stats-window`: number of minute buckets to keep in memory
- `--summary-interval`: periodic log summary interval; use `0s` to disable

## Stats endpoint

`range-porter` exposes a read-only HTTP endpoint on `GET /stats`.

Example:

```bash
curl http://127.0.0.1:9090/stats
```

The response includes:

- aggregate TCP/UDP totals
- per-port current counters
- recent minute buckets for throughput and connection peaks

## Benchmarks

The repository includes Criterion microbenchmarks for deterministic hot paths such as
port-set parsing and stats aggregation.

Run the current benchmark suite with:

```bash
cargo bench --bench core
```

## Caveats

- UDP forwarding is session-based. Idle sessions are evicted after `--udp-idle-timeout`.
- The UDP target sees the forwarder's upstream socket as the packet source, not the original client address.
- Large port ranges create one TCP listener and one UDP socket per port.
- This tool competes with other userspace forwarders on ergonomics and observability, not with kernel data-plane throughput.
