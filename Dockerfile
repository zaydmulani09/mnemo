# ── Stage 1: Builder ──────────────────────────────────────────────────────────
FROM rust:1.78-slim AS builder

# Install musl tools for static binary
RUN apt-get update && apt-get install -y \
    musl-tools \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Add musl target
RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /build

# Cache dependencies — copy manifests first
COPY Cargo.toml Cargo.lock ./
COPY crates/mnemo-core/Cargo.toml crates/mnemo-core/
COPY crates/mnemo-api/Cargo.toml crates/mnemo-api/
COPY crates/mnemo-cli/Cargo.toml crates/mnemo-cli/
COPY crates/mnemo-bench/Cargo.toml crates/mnemo-bench/

# Create stub lib/main files to cache deps without full source
RUN mkdir -p crates/mnemo-core/src && echo "pub fn stub() {}" > crates/mnemo-core/src/lib.rs
RUN mkdir -p crates/mnemo-api/src && echo "fn main() {}" > crates/mnemo-api/src/main.rs
RUN mkdir -p crates/mnemo-cli/src && echo "fn main() {}" > crates/mnemo-cli/src/main.rs
RUN mkdir -p crates/mnemo-bench/src && echo "fn main() {}" > crates/mnemo-bench/src/main.rs

# Build deps only (cached layer)
RUN cargo build --release --target x86_64-unknown-linux-musl -p mnemo-api 2>/dev/null || true

# Now copy real source
COPY crates/ crates/

# Touch to invalidate dep cache
RUN find crates -name "*.rs" -exec touch {} +

# Build real binary
RUN cargo build --release --target x86_64-unknown-linux-musl -p mnemo-api

# Strip binary
RUN strip /build/target/x86_64-unknown-linux-musl/release/mnemo-api

# ── Stage 2: Runtime ──────────────────────────────────────────────────────────
FROM scratch

# Copy CA certs for HTTPS to LLM providers
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/

# Copy binary
COPY --from=builder /build/target/x86_64-unknown-linux-musl/release/mnemo-api /mnemo-api

# Data directory for SQLite
VOLUME ["/data"]

ENV MNEMO_DB_PATH=/data/mnemo.db
ENV MNEMO_PORT=8080
ENV MNEMO_LLM_BASE_URL=http://ollama:11434/v1
ENV MNEMO_LLM_MODEL=llama3
ENV MNEMO_LLM_API_KEY=ollama
ENV MNEMO_LLM_PROVIDER=ollama

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["/mnemo-api", "--health-check"]

ENTRYPOINT ["/mnemo-api"]
