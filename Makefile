.PHONY: build run check fmt lint test clean release

build:
	cargo build

run:
	cargo run

check:
	cargo check

fmt:
	cargo fmt

lint:
	cargo clippy

test:
	cargo test

clean:
	cargo clean

# Usage: make release bump=patch (or minor, major, 0.3.0)
release:
	@./scripts/release.sh $(bump)
