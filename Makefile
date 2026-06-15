.PHONY: all build build-api build-mev check lint test fmt docker-up docker-down \
        migrate seed clean help

RUST_LOG ?= info

all: build

## Build all workspace crates
build:
	cargo build --workspace

## Build only the API server
build-api:
	cargo build --bin apex-api

## Build only the MEV engine
build-mev:
	cargo build --bin apex-mev

## Build in release mode
build-release:
	cargo build --workspace --release

## Check without building
check:
	cargo check --workspace

## Run clippy lints
lint:
	cargo clippy --workspace -- -D warnings

## Format source code
fmt:
	cargo fmt --all

## Run all tests
test:
	cargo test --workspace

## Run integration tests only
test-integration:
	cargo test --test '*' -- --test-threads=1

## Start all Docker services
docker-up:
	docker-compose up -d

## Start with live logs
docker-up-logs:
	docker-compose up

## Stop all services
docker-down:
	docker-compose down

## Stop and remove volumes
docker-clean:
	docker-compose down -v --remove-orphans

## Run database migrations (requires DATABASE_URL)
migrate:
	@echo "Running migrations against $$DATABASE_URL"
	sqlx migrate run --source database/migrations

## Watch API for development
dev-api:
	RUST_LOG=$(RUST_LOG) cargo watch -x 'run --bin apex-api'

## Watch MEV bot for development
dev-mev:
	RUST_LOG=$(RUST_LOG) cargo watch -x 'run --bin apex-mev'

## Build and push Docker images
docker-build:
	docker-compose build --parallel

## Tail API logs
logs-api:
	docker-compose logs -f api

## Tail all logs
logs:
	docker-compose logs -f

## Open Grafana in browser (macOS)
grafana:
	open http://localhost:3001

## Open frontend in browser (macOS)
open:
	open http://localhost:3000

## Generate cargo documentation
docs:
	cargo doc --workspace --no-deps --open

## Security audit
audit:
	cargo audit

clean:
	cargo clean
	docker-compose down -v --remove-orphans 2>/dev/null || true

help:
	@grep -E '^## ' Makefile | sed 's/## //'
