.PHONY: setup build-client run-client run-engine timing-check test test-client test-engine lint fmt

CARGO := cd client && cargo
UV := uv --project engine

setup:
	$(CARGO) fetch
	$(UV) sync
	$(UV) run qwertea-engine init

build-client:
	$(CARGO) build --bins

run-client:
	$(CARGO) run --release --bin qwertea

run-engine:
	$(UV) run qwertea-engine init

timing-check:
	$(CARGO) run --release --bin timing_check

test: test-client test-engine

test-client:
	$(CARGO) test

# The round-trip test needs the Rust fixture binary.
test-engine: build-client
	cd engine && uv run pytest -q

lint:
	$(CARGO) fmt --check
	$(CARGO) clippy --all-targets -- -D warnings
	cd engine && uv run ruff check . && uv run ruff format --check . && uv run mypy

fmt:
	$(CARGO) fmt
	cd engine && uv run ruff format . && uv run ruff check --fix .
