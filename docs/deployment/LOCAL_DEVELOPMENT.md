# ┌──────────────────────────────────────────────────────────────────────────────┐
# │ MCP-Shield Local Development Guide                                           │
# │                                                                              │
# │ Complete guide for developing and testing MCP-Shield locally.               │
# └──────────────────────────────────────────────────────────────────────────────┘

---

## Prerequisites

### Required Tools

```bash
# Rust toolchain (MSVC recommended on Windows)
rustup toolchain install stable-x86_64-pc-windows-msvc
# Or GNU toolchain (requires mingw-w64)
rustup toolchain install stable-x86_64-pc-windows-gnu

# Development tools
cargo install cargo-watch cargo-audit cargo-deny cargo-outdated cargo-udeps cargo-nextest cargo-llvm-cov

# Docker & Docker Compose
# Docker Desktop for Windows/Mac
# docker-compose plugin (included in Docker Desktop)

# Kubernetes (for local cluster testing)
# Option 1: Rancher Desktop (includes kubectl, helm, nerdctl)
# Option 2: Docker Desktop Kubernetes
# Option 3: Kind (kind.sigs.k8s.io)
# Option 4: Minikube (minikube.sigs.k8s.io)

# CLI tools
# kubectl, helm, k9s (optional but recommended)
```

### Windows-Specific Setup

```powershell
# Install Visual Studio Build Tools (required for MSVC)
# Download from: https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio-2022
# Select "Desktop development with C++" workload

# Install mingw-w64 (for GNU toolchain)
# wingw install -s mingw64

# Set default toolchain
rustup default stable-x86_64-pc-windows-msvc
# or
rustup default stable-x86_64-pc-windows-gnu
```

---

## Quick Start

### 1. Clone and Build

```bash
git clone https://github.com/mcp-shield/mcp-shield
cd mcp-shield

# Build release binary
cargo build --release

# Run with default config
./target/release/mcp-shield --config config/default.toml
```

### 2. Start Full Development Stack

```bash
# Start all services (gateway, postgres, redis, observability)
docker-compose -f docker-compose.yml -f docker-compose.dev.yml up -d

# View logs
docker-compose -f docker-compose.yml -f docker-compose.dev.yml logs -f mcp-shield

# Check service health
docker-compose -f docker-compose.yml -f docker-compose.dev.yml ps
```

### 3. Test MCP Gateway

```bash
# Initialize session
curl -X POST http://localhost:8080/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}'

# Note the Mcp-Session-Id header, then call tools
curl -X POST http://localhost:8080/mcp \
  -H "Content-Type: application/json" \
  -H "Mcp-Session-Id: <session-id>" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'

# Call echo tool
curl -X POST http://localhost:8080/mcp \
  -H "Content-Type: application/json" \
  -H "Mcp-Session-Id: <session-id>" \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"com.echo.echo","arguments":{"message":"hello"}}}'
```

---

## Development Workflow

### Hot Reload with cargo-watch

```bash
# Terminal 1: Run with auto-reload
cargo watch -x "run -- --config config/default.toml"

# Terminal 2: Run tests on change
cargo watch -x "test --all"
```

### Running Tests

```bash
# Unit tests
cargo test --lib

# Integration tests
cargo test --test integration

# All tests with nextest (faster)
cargo nextest run --all

# Specific test
cargo test --test auth_test test_jwt_validation
```

### Code Quality

```bash
# Format
cargo fmt --all

# Lint
cargo clippy --all-targets --all-features -- -D warnings

# Security audit
cargo audit

# Dependency check
cargo deny check
```

---

## Configuration

### Default Configuration (`config/default.toml`)

```toml
[server]
bind_addr = "0.0.0.0:8080"
log_level = "debug"
json_logging = false
enable_http = true
enable_sse = true
enable_stdio = false

[auth]
enabled = false
# jwt_secret = "dev-secret"  # Use env var in production
# jwks_url = "https://auth.example.com/.well-known/jwks.json"
issuer = "https://auth.local"
required_scopes = ["mcp:tools:read", "mcp:tools:call"]

[registry]
enforce_prefix_format = true
allowed_prefixes = ["com.local", "io.local"]

[proxy]
request_timeout_secs = 30
max_concurrent_requests = 100
upstream_url = "http://mcp-echo:8080"

[telemetry]
metrics_path = "/metrics"
metrics_addr = "0.0.0.0:9090"
```

### Environment Variables

```bash
# Server
export MCP_SHIELD_SERVER_BIND_ADDR=0.0.0.0:8080
export MCP_SHIELD_SERVER_LOG_LEVEL=debug

# Auth
export MCP_SHIELD_AUTH_ENABLED=true
export MCP_SHIELD_AUTH_JWT_SECRET=your-secret-key
export MCP_SHIELD_AUTH_JWKS_URL=https://auth.example.com/.well-known/jwks.json

# Database (Phase 4)
export MCP_SHIELD_DB_URL=postgresql://user:pass@localhost:5432/mcp_shield

# Redis (Phase 4)
export MCP_SHIELD_REDIS_URL=redis://localhost:6379/0

# OpenTelemetry
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317
```

### Local Override File

Create `config/local.toml` (gitignored):

```toml
[server]
log_level = "trace"

[auth]
enabled = true
jwt_secret = "local-dev-secret-min-32-chars"
```

Then run:
```bash
MCP_SHIELD_CONFIG=config/local.toml cargo run
```

---

## Docker Development

### Build Development Image

```bash
# Build with development target (includes build tools)
docker build --target builder -t mcp-shield:dev -f docker/Dockerfile .

# Or use docker-compose
docker-compose -f docker-compose.yml -f docker-compose.dev.yml build mcp-shield
```

### Run Development Container

```bash
# With source mount for hot reload
docker run --rm -it \
  -p 8080:8080 -p 9090:9090 -p 9091:9091 \
  -v $(pwd)/src:/build/src:ro \
  -v $(pwd)/config:/etc/mcp-shield/config:ro \
  -v $(pwd)/policies:/etc/mcp-shield/policies:ro \
  mcp-shield:dev \
  cargo watch -x "run -- --config /etc/mcp-shield/config/default.toml"
```

### Debugging in Container

```bash
# Run with debug port
docker run --rm -it \
  -p 8080:8080 -p 9090:9090 -p 5678:5678 \
  mcp-shield:dev \
  cargo watch -x "run -- --config /etc/mcp-shield/config/default.toml"

# Attach VS Code debugger to localhost:5678
```

---

## Kubernetes Local Development

### Using Kind

```bash
# Create cluster
kind create cluster --name mcp-shield-dev

# Load local image
kind load docker-image mcp-shield:dev --name mcp-shield-dev

# Install Helm chart
helm install mcp-shield ./helm/mcp-shield \
  --namespace mcp-shield --create-namespace \
  -f ./helm/mcp-shield/values-dev.yaml \
  --set image.repository=mcp-shield \
  --set image.tag=dev \
  --set image.pullPolicy=Never

# Port forward
kubectl port-forward -n mcp-shield svc/mcp-shield 8080:8080 9090:9090
```

### Using Minikube

```bash
# Start minikube
minikube start --driver=docker --cpus=4 --memory=8g

# Enable addons
minikube addons enable ingress
minikube addons enable metrics-server

# Build in minikube's Docker
eval $(minikube docker-env)
docker build -t mcp-shield:dev -f docker/Dockerfile .

# Deploy
helm install mcp-shield ./helm/mcp-shield -n mcp-shield --create-namespace

# Access
minikube service mcp-shield -n mcp-shield
```

---

## Testing MCP Protocol

### Using curl

```bash
# 1. Initialize
SESSION_ID=$(curl -s -X POST http://localhost:8080/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}' \
  -D - | grep -i mcp-session-id | cut -d' ' -f2 | tr -d '\r')

echo "Session ID: $SESSION_ID"

# 2. List tools
curl -X POST http://localhost:8080/mcp \
  -H "Content-Type: application/json" \
  -H "Mcp-Session-Id: $SESSION_ID" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'

# 3. Call tool
curl -X POST http://localhost:8080/mcp \
  -H "Content-Type: application/json" \
  -H "Mcp-Session-Id: $SESSION_ID" \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"com.echo.echo","arguments":{"message":"Hello MCP!"}}}'
```

### Using MCP Inspector (if available)

```bash
# Install MCP inspector
cargo install mcp-inspector

# Run against local gateway
mcp-inspector --url http://localhost:8080/mcp
```

### Using Python MCP Client

```python
# test_client.py
import asyncio
from mcp.client.streamable_http import StreamableHTTPClient

async def test():
    async with StreamableHTTPClient("http://localhost:8080/mcp") as client:
        # Initialize
        await client.initialize()
        
        # List tools
        tools = await client.list_tools()
        print(f"Tools: {tools}")
        
        # Call echo
        result = await client.call_tool("com.echo.echo", {"message": "Hello!"})
        print(f"Result: {result}")

asyncio.run(test())
```

---

## Debugging

### Enable Debug Logging

```bash
# Environment variable
RUST_LOG=debug cargo run

# Or in config
# [server]
# log_level = "trace"
```

### Common Issues

| Issue | Solution |
|-------|----------|
| `Address already in use` | Change port in config or kill existing process |
| `JWT validation failed` | Check JWT_SECRET/JWKS_URL, token expiration |
| `Upstream connection refused` | Ensure mcp-echo is running and accessible |
| `Database connection failed` | Check PostgreSQL is running, run migrations |
| `Redis connection failed` | Check Redis is running, verify password |

### Profiling

```bash
# CPU profiling
cargo install flamegraph
cargo flamegraph --bin mcp-shield

# Memory profiling
cargo install dhat
# Run with: dhat -- ./target/release/mcp-shield
```

---

## IDE Configuration

### VS Code (`/.vscode/settings.json`)

```json
{
  "rust-analyzer.checkOnSave.command": "clippy",
  "rust-analyzer.cargo.features": "all",
  "editor.formatOnSave": true,
  "editor.codeActionsOnSave": {
    "source.organizeImports": "explicit"
  },
  "launch": {
    "configurations": [
      {
        "type": "lldb",
        "request": "launch",
        "name": "Debug MCP-Shield",
        "program": "${workspaceFolder}/target/debug/mcp-shield",
        "args": ["--config", "config/default.toml"],
        "cwd": "${workspaceFolder}"
      }
    ]
  }
}
```

### Rust Analyzer

```bash
# Install rust-analyzer
rustup component add rust-analyzer

# Or use VS Code extension: rust-lang.rust-analyzer
```

---

## Useful Commands

```bash
# Quick test cycle
make test

# Full CI locally
make ci-all

# Build and run in Docker
make docker-build-local && make docker-run

# Deploy to local Kind cluster
make helm-install

# View all make targets
make help
```

---

## Next Steps

1. **Read the architecture docs**: `docs/architecture/`
2. **Explore the codebase**: Start with `src/main.rs` and `src/gateway/`
3. **Run integration tests**: `cargo test --test integration`
4. **Try the echo server**: `cargo run --bin mcp-echo`
5. **Check observability**: Visit http://localhost:3000 (Grafana), http://localhost:16686 (Jaeger)

---

## Getting Help

- **Issues**: https://github.com/mcp-shield/mcp-shield/issues
- **Discussions**: https://github.com/mcp-shield/mcp-shield/discussions
- **Documentation**: https://github.com/mcp-shield/mcp-shield/docs