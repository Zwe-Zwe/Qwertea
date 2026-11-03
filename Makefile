.PHONY: run setup build build-client run-client run-engine report timing-check test test-client test-engine lint fmt clean-data

CARGO := cd client && cargo
UV := uv --project engine

## One command to install everything and start the app.
run: setup build
	./client/target/release/qwertea

## Install engine dependencies (incl. the LightGBM insight model) and create the database.
setup:
	$(UV) sync --all-extras
	$(UV) run qwertea-engine init

## Build the app (release).
build:
	$(CARGO) build --release --bin qwertea

build-client:
	$(CARGO) build --bins

run-client:
	$(CARGO) run --release --bin qwertea

## Run the engine in the foreground (the app normally starts it for you).
run-engine:
	$(UV) run qwertea-engine serve

report:
	$(UV) run qwertea-engine report

timing-check:
	$(CARGO) run --release --bin timing_check

test: test-client test-engine

test-client:
	$(CARGO) test

# The round-trip test needs the Rust fixture binary.
test-engine: build-client
	cd engine && uv run --all-extras pytest -q

lint:
	$(CARGO) fmt --check
	$(CARGO) clippy --all-targets -- -D warnings
	$(CARGO) clippy --all-targets --features evdev-capture -- -D warnings
	cd engine && uv run ruff check . && uv run ruff format --check . && uv run mypy

fmt:
	$(CARGO) fmt
	cd engine && uv run ruff format . && uv run ruff check --fix .

## Delete your local typing data (asks first).
clean-data:
	@read -p "Delete data/ (all your typing history)? [y/N] " a && [ "$$a" = y ] && rm -rf data/qwertea.db* data/reports data/engine.* || echo "kept"
