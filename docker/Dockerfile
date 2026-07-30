# ── Build stage ────────────────────────────────────────────────────────
FROM rust:1.89-bookworm AS builder

WORKDIR /build

# Install required system dependencies for building
# librdkafka-dev is needed for the rdkafka crate (Phase 2 telemetry)
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests first to leverage Docker layer caching
COPY Cargo.toml Cargo.lock* ./

# Create a dummy main.rs so we can build dependencies first
RUN mkdir -p src && echo "fn main() {}" > src/main.rs && \
    echo "pub fn dummy() {}" > src/lib.rs

# Build dependencies (this layer is cached unless Cargo.toml changes)
RUN cargo build --release || true

# Copy the actual source code
COPY src/ ./src/
COPY config/ ./config/
COPY policies/ ./policies/

# Build the actual binary
RUN touch src/main.rs src/lib.rs && \
    cargo build --release

# ── Runtime stage ─────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

# Install minimal runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 65532 nonroot \
    && useradd --uid 65532 --gid 65532 --create-home --home-dir /home/nonroot nonroot

WORKDIR /home/nonroot

# Copy the binary from the build stage
COPY --from=builder /build/target/release/mcp-shield /usr/local/bin/mcp-shield

# Copy default configuration and policies
COPY --from=builder /build/config/ /etc/mcp-shield/config/
COPY --from=builder /build/policies/ /etc/mcp-shield/policies/

# Switch to non-root user
USER nonroot

# Expose the HTTP port
EXPOSE 8080 9090

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["/usr/local/bin/mcp-shield", "--health-check"] || exit 1

# Set environment variables
ENV MCP_SHIELD_CONFIG=/etc/mcp-shield/config/default.toml \
    RUST_LOG=info \
    RUST_BACKTRACE=1

ENTRYPOINT ["/usr/local/bin/mcp-shield"]
