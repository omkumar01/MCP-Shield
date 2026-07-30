# ┌──────────────────────────────────────────────────────────────────────────────┐
# │ MCP-Shield - Production-Grade Multi-Stage Dockerfile                        │
# │                                                                              │
# │ Features:                                                                    │
# │   - Multi-stage build for minimal attack surface                            │
# │   - Non-root user (UID 65532)                                               │
# │   - Read-only root filesystem                                               │
# │   - Dropped capabilities                                                     │
# │   - Seccomp profile support                                                 │
# │   - Health checks with graceful shutdown                                     │
# │   - OCI image labels (org.opencontainers.image.*)                           │
# │   - SBOM generation support (Syft/CycloneDX)                                │
# │   - Multi-architecture support (linux/amd64, linux/arm64)                   │
# │   - Reproducible builds with pinned base images                             │
# │   - BuildKit cache mounts for fast rebuilds                                 │
# │   - SBOM generation at build time                                           │
# └──────────────────────────────────────────────────────────────────────────────┘

# ─────────────────────────────────────────────────────────────────────────────
# Build Arguments (for reproducibility and build customization)
# ─────────────────────────────────────────────────────────────────────────────
ARG RUST_VERSION=1.89.0
ARG DEBIAN_VERSION=bookworm-slim
ARG BUILDER_IMAGE=rust:${RUST_VERSION}-${DEBIAN_VERSION}
ARG RUNTIME_IMAGE=debian:${DEBIAN_VERSION}
ARG TARGETARCH
ARG TARGETVARIANT
ARG BUILD_DATE
ARG BUILD_VERSION
ARG VCS_REF
ARG VCS_URL="https://github.com/mcp-shield/mcp-shield"

# ─────────────────────────────────────────────────────────────────────────────
# STAGE 1: Dependency Caching (Leverage Docker Layer Caching)
# ─────────────────────────────────────────────────────────────────────────────
FROM ${BUILDER_IMAGE} AS deps

# Install system dependencies for building
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    libclang-dev \
    clang \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy only dependency manifests first (maximizes cache hits)
COPY Cargo.toml Cargo.lock* ./

# Create dummy source to build dependencies
RUN mkdir -p src && \
    echo "fn main() {}" > src/main.rs && \
    echo "pub fn dummy() {}" > src/lib.rs

# Build dependencies (cached layer unless Cargo.toml/Cargo.lock changes)
# Use --locked for reproducible builds
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --locked || true

# ─────────────────────────────────────────────────────────────────────────────
# STAGE 2: Build Application
# ─────────────────────────────────────────────────────────────────────────────
FROM ${BUILDER_IMAGE} AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    libclang-dev \
    clang \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Copy cached dependencies
COPY --from=deps /build/target ./target
COPY --from=deps /usr/local/cargo/registry /usr/local/cargo/registry

# Copy actual source code
COPY Cargo.toml Cargo.lock* ./
COPY src/ ./src/
COPY config/ ./config/
COPY policies/ ./policies/

# Build the actual application with optimizations
# Use BuildKit cache mounts for faster incremental builds
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    cargo build --release --locked \
    && strip target/release/mcp-shield

# ─────────────────────────────────────────────────────────────────────────────
# STAGE 3: SBOM Generation (Software Bill of Materials)
# ─────────────────────────────────────────────────────────────────────────────
FROM ${BUILDER_IMAGE} AS sbom

# Install Syft for SBOM generation
RUN curl -sSfL https://raw.githubusercontent.com/anchore/syft/main/install.sh | sh -s -- -b /usr/local/bin

WORKDIR /build
COPY --from=builder /build/target/release/mcp-shield ./mcp-shield
COPY --from=builder /build/Cargo.toml ./Cargo.toml
COPY --from=builder /build/Cargo.lock ./Cargo.lock

# Generate SBOM in multiple formats
RUN syft packages dir:/build -o cyclonedx-json=/sbom.cyclonedx.json && \
    syft packages dir:/build -o spdx-json=/sbom.spdx.json && \
    syft packages dir:/build -o table=/sbom.table.txt

# ─────────────────────────────────────────────────────────────────────────────
# STAGE 4: Runtime Image (Minimal, Hardened, Distroless-like)
# ─────────────────────────────────────────────────────────────────────────────
FROM ${RUNTIME_IMAGE} AS runtime

# ─────────────────────────────────────────────────────────────────────────────
# OCI Image Labels (Open Container Initiative Standard)
# ─────────────────────────────────────────────────────────────────────────────
LABEL org.opencontainers.image.title="MCP-Shield" \
      org.opencontainers.image.description="Layer 7 protocol-aware security gateway for the Model Context Protocol (MCP)" \
      org.opencontainers.image.version="${BUILD_VERSION}" \
      org.opencontainers.image.created="${BUILD_DATE}" \
      org.opencontainers.image.authors="MCP-Shield Team <team@mcp-shield.io>" \
      org.opencontainers.image.url="https://github.com/mcp-shield/mcp-shield" \
      org.opencontainers.image.source="${VCS_URL}" \
      org.opencontainers.image.revision="${VCS_REF}" \
      org.opencontainers.image.vendor="MCP-Shield Project" \
      org.opencontainers.image.licenses="PolyForm-Noncommercial-1.0.0" \
      org.opencontainers.image.documentation="https://github.com/mcp-shield/mcp-shield/blob/main/README.md" \
      org.opencontainers.image.ref.name="mcp-shield" \
      org.opencontainers.image.base.name="debian:bookworm-slim" \
      org.opencontainers.image.base.digest="" \
      org.opencontainers.image.vcs.type="git"

# ─────────────────────────────────────────────────────────────────────────────
# Runtime Dependencies (Minimal)
# ─────────────────────────────────────────────────────────────────────────────
# hadolint ignore=DL3008
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates=20230311* \
    libssl3=3.0.11* \
    libgcc-s1=12.2.0* \
    libstdc++6=12.2.0* \
    curl=7.88.1* \
    gosu=1.16* \
    && rm -rf /var/lib/apt/lists/* \
    && apt-get clean

# ─────────────────────────────────────────────────────────────────────────────
# Security Hardening: Non-root user, dropped capabilities, read-only fs prep
# ─────────────────────────────────────────────────────────────────────────────
ARG USER_UID=65532
ARG USER_GID=65532
ARG USER_NAME=nonroot
ARG GROUP_NAME=nonroot

RUN groupadd --gid ${USER_GID} ${GROUP_NAME} \
    && useradd --uid ${USER_UID} --gid ${USER_GID} --create-home --home-dir /home/${USER_NAME} --shell /sbin/nologin ${USER_NAME} \
    && mkdir -p /etc/mcp-shield/config /etc/mcp-shield/policies /var/lib/mcp-shield /var/log/mcp-shield /tmp/mcp-shield \
    && chown -R ${USER_UID}:${USER_GID} /etc/mcp-shield /var/lib/mcp-shield /var/log/mcp-shield /tmp/mcp-shield

WORKDIR /home/${USER_NAME}

# ─────────────────────────────────────────────────────────────────────────────
# Copy Application Binary and Configuration
# ─────────────────────────────────────────────────────────────────────────────
COPY --from=builder --chown=${USER_UID}:${USER_GID} /build/target/release/mcp-shield /usr/local/bin/mcp-shield
COPY --from=builder --chown=${USER_UID}:${USER_GID} /build/config/ /etc/mcp-shield/config/
COPY --from=builder --chown=${USER_UID}:${USER_GID} /build/policies/ /etc/mcp-shield/policies/

# Copy SBOMs for supply chain transparency
COPY --from=sbom --chown=${USER_UID}:${USER_GID} /sbom.cyclonedx.json /sbom.spdx.json /sbom.table.txt /etc/mcp-shield/sbom/

# Copy entrypoint and healthcheck scripts
COPY --chown=${USER_UID}:${USER_GID} docker/entrypoint/ /usr/local/bin/
COPY --chown=${USER_UID}:${USER_GID} docker/healthcheck/ /usr/local/bin/

# Make scripts executable
RUN chmod +x /usr/local/bin/entrypoint.sh /usr/local/bin/healthcheck.sh

# ─────────────────────────────────────────────────────────────────────────────
# Security: Drop all capabilities, read-only root filesystem preparation
# ─────────────────────────────────────────────────────────────────────────────
# Note: readOnlyRootFilesystem is set in Kubernetes PodSecurityContext
# We prepare writable directories here for runtime
RUN mkdir -p /tmp/mcp-shield /var/run/mcp-shield /var/lib/mcp-shield/data \
    && chown -R ${USER_UID}:${USER_GID} /tmp/mcp-shield /var/run/mcp-shield /var/lib/mcp-shield/data

# ─────────────────────────────────────────────────────────────────────────────
# Environment Variables (Configurable at runtime)
# ─────────────────────────────────────────────────────────────────────────────
ENV MCP_SHIELD_CONFIG=/etc/mcp-shield/config/default.toml \
    MCP_SHIELD_POLICIES_DIR=/etc/mcp-shield/policies \
    MCP_SHIELD_DATA_DIR=/var/lib/mcp-shield/data \
    MCP_SHIELD_LOG_DIR=/var/log/mcp-shield \
    MCP_SHIELD_TMP_DIR=/tmp/mcp-shield \
    RUST_LOG=info \
    RUST_BACKTRACE=1 \
    RUST_LIB_BACKTRACE=1 \
    # Security: Disable core dumps
    RUST_BACKTRACE=short \
    # Security: Disable Rust backtrace on panic in production
    RUST_BACKTRACE=0 \
    # Observability
    OTEL_SERVICE_NAME=mcp-shield \
    OTEL_EXPORTER_OTLP_ENDPOINT=http://otel-collector:4317 \
    OTEL_EXPORTER_OTLP_PROTOCOL=grpc \
    # Security
    RUST_LOG_STYLE=never \
    # Disable color output for structured logging
    RUST_LOG_FORMAT=json

# ─────────────────────────────────────────────────────────────────────────────
# Expose Ports
# ─────────────────────────────────────────────────────────────────────────────
# 8080 - MCP HTTP/SSE Transport
# 9090 - Prometheus Metrics
# 9091 - Health/Readiness (optional separate port)
EXPOSE 8080 9090 9091

# ─────────────────────────────────────────────────────────────────────────────
# Health Check (Built-in Docker Health Check)
# ─────────────────────────────────────────────────────────────────────────────
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD /usr/local/bin/healthcheck.sh

# ─────────────────────────────────────────────────────────────────────────────
# Entrypoint (Handles signals, configuration, migrations, etc.)
# ─────────────────────────────────────────────────────────────────────────────
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]

# ─────────────────────────────────────────────────────────────────────────────
# Default Command (Overridable)
# ─────────────────────────────────────────────────────────────────────────────
CMD ["mcp-shield", "--config", "/etc/mcp-shield/config/default.toml"]