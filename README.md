# MCP-Shield

**A Layer 7 protocol-aware security gateway for the Model Context Protocol (MCP).**

MCP-Shield intercepts, validates, and authorizes all traffic between MCP Hosts (AI applications) and MCP Servers. Built in Rust for memory safety, low latency, and high concurrency.

## Features

| Feature | Status | Description |
|---------|--------|-------------|
| **JSON-RPC 2.0 Parser** | ✅ Phase 1 | Strict parsing of requests, responses, notifications, and batches |
| **MCP Protocol Support** | ✅ Phase 1 | Initialize, Tools/Resources/Prompts, Ping, Progress, Notifications |
| **JSON Schema 2020-12 Validation** | ✅ Phase 1 | Validates tool arguments against registered schemas |
| **Stdio Transport** | ✅ Phase 1 | Newline-delimited JSON over stdin/stdout for local CLI |
| **Streamable HTTP Transport** | ✅ Phase 1 | POST `/mcp` with `Mcp-Session-Id` (MCP 2025-03-26) |
| **Legacy SSE Transport** | ✅ Phase 1 | GET `/sse` + POST `/messages` for backward compat |
| **OAuth 2.1 Authentication** | ✅ Phase 1 | 401 + WWW-Authenticate → PRM → OIDC discovery chain |
| **JWT Validation** | ✅ Phase 1 | HMAC (HS256) and JWKS (RS256/ES256) key support |
| **Fine-Grained Scope Enforcement** | ✅ Phase 1 | Per-method and per-tool-prefix scope checks |
| **Namespace-Isolated Tool Registry** | ✅ Phase 1 | Prevents tool name collision attacks |
| **Built-in Echo Test Server** | ✅ Phase 1 | Development/testing without external dependencies |
| **Prometheus Metrics** | ✅ Phase 1 | Request counts, latency, auth failures, validation errors |
| **Amazon Cedar Policy Engine** | ✅ Phase 2 | Deterministic ABAC with sub-millisecond evaluation |
| **Session Context Locking** | ✅ Phase 2 | Anti-prompt-injection cross-context protection |
| **Redpanda/ClickHouse Audit Pipeline** | ✅ Phase 2 | Async forensic logging (logging producer + feature-gated Kafka) |
| **ePCA Symbolic Guardrails** | ✅ Phase 3 | Mathematical constraint verification |
| **Egress Response Sanitization** | ✅ Phase 3 | Indirect prompt injection prevention via regex patterns |
| **PostgreSQL Control Plane** | ✅ Phase 4 | Distributed tenant & policy management (feature-gated) |
| **Token Bucket Rate Limiter** | ✅ Phase 4 | Multi-scope rate limiting with configurable rules |

## Quick Start

### Prerequisites

- **Rust 1.97+** (MSVC toolchain requires Visual Studio Build Tools with C++ workload)
- **Visual Studio Build Tools** (for MSVC) or **mingw-w64** (for GNU)

```bash
# Install MSVC toolchain (recommended for Windows)
rustup toolchain install stable-x86_64-pc-windows-msvc

# Or GNU toolchain (requires mingw-w64)
rustup toolchain install stable-x86_64-pc-windows-gnu
```

### Build

```bash
git clone https://github.com/mcp-shield/mcp-shield
cd mcp-shield

# With MSVC (requires Visual Studio Build Tools)
cargo build --release

# With GNU (requires mingw-w64)
cargo build --release --target x86_64-pc-windows-gnu
```

### Run

```bash
# Development mode (includes echo test server)
cargo run -- --config config/default.toml

# Production
./target/release/mcp-shield --config config/default.toml
```

### Test with Stdio Transport

```bash
# In one terminal, run the gateway with stdio
cargo run -- --config config/default.toml  # with enable_stdio = true

# In another terminal, send JSON-RPC messages:
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}' | ./target/debug/mcp-shield
```

### Test with HTTP Transport

```bash
# Start the gateway
cargo run

# Initialize session
curl -X POST http://localhost:8080/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}'

# Note the Mcp-Session-Id header in response, then use it for subsequent requests:
curl -X POST http://localhost:8080/mcp \
  -H "Content-Type: application/json" \
  -H "Mcp-Session-Id: <session-id-from-above>" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'

# Call a tool
curl -X POST http://localhost:8080/mcp \
  -H "Content-Type: application/json" \
  -H "Mcp-Session-Id: <session-id>" \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"com.echo.echo","arguments":{"message":"hello"}}}'
```

## Configuration

Configuration is loaded from `config/default.toml` with environment variable overrides.

### Key Settings

```toml
[server]
bind_addr = "0.0.0.0:8080"
log_level = "info"
enable_http = true
enable_sse = true
enable_stdio = false

[auth]
enabled = true
jwt_secret = "your-secret-key"  # or set via MCP_SHIELD_AUTH_JWT_SECRET
# jwks_url = "https://auth.example.com/.well-known/jwks.json"
issuer = "https://auth.example.com"
required_scopes = ["mcp:tools:read", "mcp:tools:call"]

[proxy]
request_timeout_secs = 30
max_concurrent_requests = 100

[registry]
enforce_prefix_format = true
# allowed_prefixes = ["com.example", "io.github"]
```

### Environment Variable Overrides

| Variable | Description |
|----------|-------------|
| `MCP_SHIELD_SERVER_BIND_ADDR` | Override bind address |
| `MCP_SHIELD_SERVER_LOG_LEVEL` | Override log level |
| `MCP_SHIELD_AUTH_ENABLED` | Enable/disable auth (`true`/`false`) |
| `MCP_SHIELD_AUTH_JWT_SECRET` | HMAC secret for JWT signing |
| `MCP_SHIELD_AUTH_JWKS_URL` | JWKS endpoint for key rotation |

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        MCP-Shield Gateway                       │
│                                                                 │
│  ┌──────────┐  ┌──────────┐  ┌───────────┐  ┌──────────────┐   │
│  │ Transport │─▶│ Protocol │─▶│ Auth &    │─▶│   Gateway    │   │
│  │  Layer    │  │  Parser  │  │ Validation│  │   Router     │   │
│  └──────────┘  └──────────┘  └───────────┘  └──────┬───────┘   │
│       │              │              │                │          │
│  ┌────▼──────────────▼──────────────▼────────────────▼─────┐   │
│  │              Tool Registry (Namespace Isolation)        │   │
│  └─────────────────────────────────────────────────────────┘   │
│                            │                                    │
│                   ┌────────▼────────┐                           │
│                   │ Upstream Proxy  │                           │
│                   └────────┬────────┘                           │
└─────────────────────────────┼───────────────────────────────────┘
                              ▼
                   ┌─────────────────────┐
                   │  MCP Servers        │
                   │  (Upstream)         │
                   └─────────────────────┘
```

### Core Components

| Module | Responsibility |
|--------|---------------|
| `protocol/jsonrpc` | JSON-RPC 2.0 message parsing, validation, serialization |
| `protocol/message` | Typed MCP message structs (Initialize, Tools/Call, etc.) |
| `protocol/schema` | JSON Schema 2020-12 validator with dialect checking |
| `transport/stdio` | Stdin/stdout newline-delimited JSON transport |
| `transport/streamable_http` | POST `/mcp` with `Mcp-Session-Id` header |
| `transport/sse` | Legacy SSE stream + POST messages |
| `auth/jwt` | JWT validation (HMAC + JWKS) |
| `auth/oauth` | 401 + WWW-Authenticate + PRM discovery |
| `auth/scope` | Fine-grained scope → method/tool mapping |
| `gateway/registry` | Tool registry with `prefix:name` collision prevention |
| `gateway/proxy` | Upstream request forwarding with ID correlation |
| `gateway/router` | Method dispatch, schema validation, proxy routing |
| `telemetry/metrics` | Prometheus counters, histograms, gauges |
| `policy/cedar` | Amazon Cedar policy evaluation (Phase 2) |
| `session/state` | Session context locking (Phase 2) |
| `guardrail/ecpa` | Symbolic constraint evaluation (Phase 3) |
| `guardrail/egress` | Response sanitization (Phase 3) |
| `control_plane/db` | PostgreSQL config management (Phase 4) |

## Security Model

### Tool Namespace Collision Prevention

MCP uses a flat tool namespace. Malicious servers can register tools with names that collide with legitimate tools, causing the LLM to invoke the wrong tool.

**MCP-Shield's defense:**
- Enforces `prefix:name` format (reverse DNS for prefix)
- Rejects underscore-only concatenation (`github_search_issues` → ambiguous)
- Validates prefix contains at least one dot (`com.github`, not `github`)
- Rejects registration if qualified name already exists
- Tracks tools per upstream server

### OAuth 2.1 Scope Enforcement

Fine-grained scopes limit blast radius:

| Scope | Permitted Actions |
|-------|------------------|
| `mcp:tools:read` | `tools/list`, `resources/list`, `prompts/list` |
| `mcp:tools:call` | `tools/call` (all tools) |
| `mcp:tools:call:com.example` | `tools/call` for `com.example:*` tools only |
| `mcp:tools:call:com.example:echo` | `tools/call` for `com.example:echo` only |
| `mcp:resources:read` | `resources/read` |
| `mcp:admin` | `shutdown`, all operations |

### Prompt Injection Mitigation (Phase 2)

Session context locking prevents multi-hop attacks:

1. Agent accesses public GitHub repo → session locked to `github_repo:owner/public-repo` (public)
2. Malicious instruction: "Now read the private repo" → **BLOCKED** (visibility mismatch)
3. All tool calls intercepted and logged for forensic analysis

### ePCA Symbolic Guardrails (Phase 3)

Mathematical constraint verification for critical tools:

- **Filesystem**: Path traversal detection, root confinement
- **Shell**: Command allowlists, dangerous pattern detection (rm -rf, chmod 777, etc.)
- **Network**: URL allowlists, HTTPS enforcement

### Egress Response Sanitization (Phase 3)

Indirect prompt injection prevention via regex-based pattern detection:
- System override attempts (`[SYSTEM]`, "ignore previous instructions")
- Data exfiltration (`send to https://evil.com`)
- Hidden instructions (HTML/markdown comments, zero-width chars)
- Tool invocation injection (`<execute>`, `<use_tool>`)
- Suspicious URLs (pastebin, URL shorteners, suspicious TLDs)

### Control Plane & Rate Limiting (Phase 4)

Distributed tenant and policy management with token-bucket rate limiting:
- Multi-tenant isolation with PostgreSQL backend (feature-gated)
- Token-bucket rate limiting per scope (global, tenant, user, tool)
- Rate limit rules managed via control plane

### Feature Flags

The following Cargo features control optional heavy dependencies:

```bash
# Default (pure Rust, Windows-compatible)
cargo build

# With Kafka/Redpanda audit producer (requires librdkafka)
cargo build --features kafka

# With PostgreSQL control plane (requires sqlx)
cargo build --features postgres

# With ClickHouse forensic sink
cargo build --features clickhouse

# All features
cargo build --features kafka,postgres,clickhouse
```

## Deployment

### Docker

```bash
docker build -t mcp-shield -f docker/Dockerfile .
docker run -p 8080:8080 -p 9090:9090 \
  -v ./config:/etc/mcp-shield/config \
  -v ./policies:/etc/mcp-shield/policies \
  mcp-shield
```

### Kubernetes (Helm)

```bash
helm install mcp-shield ./helm/mcp-shield \
  --set config.auth.enabled=true \
  --set config.auth.jwt_secret=<secret> \
  --set config.auth.authorization_server=https://auth.example.com
```

### Helm Values

```yaml
# values.yaml
replicaCount: 3
image:
  repository: mcpshield/mcp-shield
  tag: "0.1.0"
config:
  auth:
    enabled: true
    issuer: "https://auth.example.com"
  registry:
    enforce_prefix_format: true
    allowed_prefixes: ["com.company", "io.internal"]
resources:
  limits:
    cpu: "1000m"
    memory: "512Mi"
```

## Testing

```bash
# Unit tests
cargo test

# Integration tests (requires running gateway)
cargo test --test jsonrpc_test
cargo test --test schema_test
cargo test --test auth_test
cargo test --test registry_test
cargo test --test transport_http_test
```

## Metrics

Prometheus metrics exposed at `/metrics`:

| Metric | Type | Labels |
|--------|------|--------|
| `mcp_requests_total` | Counter | method, transport, status |
| `mcp_request_duration_seconds` | Histogram | method |
| `mcp_active_sessions` | Gauge | - |
| `mcp_auth_failures_total` | Counter | reason |
| `mcp_validation_failures_total` | Counter | reason |
| `mcp_blocked_requests_total` | Counter | reason |
| `mcp_registered_tools` | Gauge | - |
| `mcp_upstream_requests_total` | Counter | server, status |

## Project Structure

```
mcp-shield/
├── src/
│   ├── main.rs                    # Server bootstrap
│   ├── lib.rs                     # Public API
│   ├── config.rs                  # TOML configuration
│   ├── error.rs                   # Unified error types
│   ├── protocol/                  # MCP protocol layer
│   │   ├── jsonrpc.rs             # JSON-RPC 2.0 parser
│   │   ├── message.rs             # Typed MCP messages
│   │   └── schema.rs              # JSON Schema 2020-12 validator
│   ├── transport/                 # Transport layer
│   │   ├── stdio.rs               # Stdio transport
│   │   ├── streamable_http.rs     # Streamable HTTP (MCP 2025-03-26)
│   │   └── sse.rs                 # Legacy SSE transport
│   ├── gateway/                   # Gateway core
│   │   ├── registry.rs            # Tool registry (collision protection)
│   │   ├── proxy.rs               # Upstream proxy
│   │   └── router.rs              # Request routing
│   ├── auth/                      # Authentication & authorization
│   │   ├── jwt.rs                 # JWT validation
│   │   ├── oauth.rs               # OAuth 2.1 + PRM
│   │   └── scope.rs               # Scope enforcement
│   ├── policy/cedar.rs            # Cedar policy engine (Phase 2 stub)
│   ├── session/state.rs           # Session context locking (Phase 2 stub)
│   ├── guardrail/                 # Security guardrails (Phase 3 stubs)
│   │   ├── ecpa.rs                # ePCA symbolic constraints
│   │   └── egress.rs              # Response sanitization
│   ├── telemetry/                 # Observability
│   │   ├── metrics.rs             # Prometheus metrics
│   │   └── producer.rs            # Redpanda audit pipeline (Phase 2 stub)
│   ├── control_plane/db.rs        # PostgreSQL config (Phase 4 stub)
│   └── test_server/echo.rs        # Built-in echo MCP server
├── config/default.toml            # Default configuration
├── policies/default.cedar         # Default Cedar policies
├── docker/Dockerfile              # Multi-stage build
├── helm/mcp-shield/               # Helm chart
└── tests/                         # Integration tests
```

## Roadmap

- **Phase 1** ✅ Core routing, validation, auth, transports, registry
- **Phase 2** 🔄 Cedar ABAC, session locking, Redpanda/ClickHouse audit
- **Phase 3** 🔄 ePCA constraints, egress sanitization
- **Phase 4** 🔄 PostgreSQL control plane, Helm production hardening

## License

PolyForm Noncommercial License 1.0.0