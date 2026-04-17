# Repository Guidelines

## Project Structure & Module Organization

```
src/
  main.rs          - Entry point, CLI parsing, tracing init, shutdown signal
  lib.rs           - Re-exports public API (RuntimeConfig, start, RunningApp)
  cli.rs           - Clap argument definitions and duration parser
  config.rs        - RuntimeConfig validation and Cli→config conversion
  portset.rs       - Port expression parser (e.g. "80,443,20000-50000")
  runtime.rs       - Tokio runtime bootstrap, listener spawning, shutdown
  socket.rs        - Socket option helpers (SO_REUSEADDR, etc.)
  udp_session.rs   - UDP session tracking with idle eviction
  http.rs          - Axum stats HTTP endpoint handler
  forward/         - TCP and UDP bidirectional copy loops
    tcp.rs, udp.rs
  listener/        - Per-port TCP listener and UDP socket setup
    tcp.rs, udp.rs
  stats/           - In-memory minute-bucket counters and port-level stats
    bucket.rs, mod.rs, port.rs
tests/
  common/mod.rs    - Shared test helpers
  portset.rs       - Port expression parsing tests
  stats_rollup.rs  - Stats bucket rollup tests
  tcp_forward.rs   - TCP forwarding integration tests
  udp_forward.rs   - UDP forwarding integration tests
```

## Build, Test, and Development Commands

- `cargo build` — compile the project
- `cargo test` — run all unit and integration tests
- `cargo run -- --listen-ports 80,443 --target 127.0.0.1:8080` — run locally with required flags
- `cargo clippy` — lint with Clippy
- `RUST_LOG=debug cargo run -- ...` — enable verbose tracing output

## Coding Style & Naming Conventions

- Rust 2024 edition, idiomatic async Rust with Tokio.
- `snake_case` for functions, variables, and modules; `PascalCase` for types and structs.
- Use `anyhow::Result` for error handling; `bail!` for validation failures.
- Imports grouped: std, external crates, crate-internal.
- No nightly features; stable Rust only.

## Testing Guidelines

- Tests live in `tests/` (integration) and within source files using `#[cfg(test)]` modules (unit).
- Framework: built-in `#[test]` with Tokio's `#[tokio::test]` for async tests.
- Run with `cargo test`. Filter with `cargo test <test_name>`.
- Cover both happy paths and validation error paths.

## Commit & Pull Request Guidelines

- Commit messages: imperative mood, lowercase start (e.g. "add UDP session eviction").
- Keep PRs focused on a single concern; describe the change and motivation.
- Ensure `cargo test` and `cargo clippy` pass before submitting.
