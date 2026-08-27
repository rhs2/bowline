# One entry point for everything. `make help` lists the targets.
SHELL := /bin/bash
export PATH := $(HOME)/.cargo/bin:$(PATH)

# Use the analytics virtualenv when it exists (`make venv` creates it), so the
# Python targets do not depend on whatever `python3` happens to be on PATH.
PY := $(shell [ -x $(CURDIR)/analytics/.venv/bin/python ] && echo $(CURDIR)/analytics/.venv/bin/python || echo python3)

.PHONY: help up down migrate seed venv api web billing analytics notify test smoke lint fmt

help:            ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  %-12s %s\n", $$1, $$2}'

up:              ## Start Postgres, Redis, Mailpit, MinIO
	docker compose up -d postgres redis mailpit minio minio-init

down:            ## Stop everything and keep the data volumes
	docker compose --profile app down

migrate:         ## Apply pending migrations (the API also does this on start)
	cd api && cargo run --quiet --bin migrate

seed:            ## Load the demo company (260 people, customers, shipments, ledger)
	cd api && cargo run --quiet --bin seed

venv:            ## Create the analytics virtualenv and install its dependencies
	cd analytics && python3 -m venv .venv && \
	  .venv/bin/pip install -q --upgrade pip && \
	  .venv/bin/pip install -q -r requirements.txt -r requirements-dev.txt

api:             ## Run the Rust API on :8080
	cd api && cargo run --bin bowline-api

web:             ## Run the Next.js app on :3000
	cd web && npm run dev

billing:         ## Run the Java billing service on :8081
	cd billing && ./mvnw -q spring-boot:run

analytics:       ## Run the Python analytics service on :8082
	cd analytics && $(PY) -m analytics.main

notify:          ## Run the Go outbox worker
	cd tools && go run ./cmd/notify

test:            ## Run every test suite
	cd api && cargo test
	cd web && npm run typecheck && npm test -- --run
	cd billing && ./mvnw -B -q test
	cd analytics && $(PY) -m pytest -q
	cd tools && go test ./...

smoke:           ## End-to-end scenario against the local stack
	./scripts/smoke.sh

lint:            ## Linters for every service
	cd api && cargo fmt --check && cargo clippy --all-targets -- -D warnings
	cd web && npm run lint
	cd analytics && $(PY) -m ruff check .
	cd tools && go vet ./...

fmt:             ## Format everything
	cd api && cargo fmt
	cd web && npx prettier --write .
	cd analytics && $(PY) -m ruff format .
	cd tools && gofmt -w .
