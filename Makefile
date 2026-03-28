.PHONY: build test run docker-build docker-run docker-up docker-down clean fmt lint sdk-install sdk-test all-tests

build:
	cargo build --workspace

test:
	cargo test --workspace

run:
	cargo run -p mnemo-api

fmt:
	cargo fmt --all

lint:
	cargo clippy --workspace -- -D warnings

docker-build:
	docker build -t mnemo:latest .

docker-run:
	docker run -p 8080:8080 -v mnemo-data:/data mnemo:latest

docker-up:
	docker compose up -d

docker-down:
	docker compose down

clean:
	cargo clean
	docker compose down -v

sdk-install:
	cd sdk/python && pip install -e ".[dev]"

sdk-test:
	cd sdk/python && pytest tests/ -v

all-tests: test sdk-test

coverage:
	cargo llvm-cov --workspace --html --output-dir coverage/
	@echo "Coverage report at coverage/index.html"

coverage-summary:
	cargo llvm-cov --workspace --summary-only
