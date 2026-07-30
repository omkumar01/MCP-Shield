
# MCP-Shield: Layer 7 Security Gateway — Implementation Plan

## Architecture Overview

MCP-Shield is a Rust-based security gateway that sits between MCP Hosts (AI apps) and MCP Servers. It intercepts, validates, authenticates, and authorizes all JSON-RPC 2.0 traffic conforming to the MCP specification (2025-03-26).

```
MCP Host (Client)
       │
       ▼
┌─────────────────────────────────────────────┐
│  MCP-Shield Gateway                        │
│                                             │
│  ┌──────────┐  ┌──────────┐  ┌───────────┐ │
│  │ Transport │  │ Protocol │  │ Auth &    │ │
│  │ Layer     │→│ Parser   │→│ Validation │ │
│  └──────────┘  └──────────┘  └───────────┘ │
│                                   │         │
│  ┌──────────┐  ┌──────────┐      ▼         │
│  │ Tool     │  │ Session  │  ┌───────────┐ │
│  │ Registry │  │ Manager  │  │ Policy    │ │
│  └──────────┘  └──────────┘  │ (Cedar)   │ │
│       │            │         └───────────┘ │
│       ▼            ▼                  │    │
│  ┌──────────────────────────┐         │    │
│  │ Proxy / Upstream Router  │◄────────┘    │
│  └──────────────────────────┘              │
│              │                              │
└──────────────┼──────────────────────────────┘
               ▼
     MCP Servers (Upstream)
```

---

## Project Structure (Full Scaffold)

```
mcp-shield/
├── Cargo.toml                    # Workspace + dependencies
├── README.md                     # Architecture docs + usage
├── config/
│   └── default.toml              # Default configuration
├── docker/
│   ├── Dockerfile                 # Multi-stage Rust build
│   └── .dockerignore
├── helm/
│   └── mcp-shield/
│       ├── Chart.yaml
│       ├── values.yaml
│       ├── values-dev.yaml
│       └── templates/
│           ├── deployment.yaml
│           ├── service.yaml
│           ├── configmap.yaml
│           ├── serviceaccount.yaml
│           └── _helpers.tpl
├── policies/
│   └── default.cedar             # Cedar policy template
├── src/
│   ├── main.rs                   # Entry point, server bootstrap
│   ├── lib.rs                    # Public API exports
│   ├── config.rs                  # Typed configuration from TOML
│   ├── error.rs                  # Unified error types (McpError)
│   │
│   ├── protocol/                  # [Phase 1 - FULL]
│   │   ├── mod.rs
│   │   ├── jsonrpc.rs            # JSON-RPC 2.0 parser/serializer
│   │   ├── message.rs            # MCP message types (Initialize, Tools/Call, etc.)
│   │   └── schema.rs             # JSON Schema 2020-12 validation engine
│   │
│   ├── transport/                 # [Phase 1 - FULL]
│   │   ├── mod.rs
│   │   ├── stdio.rs              # Stdio: newline-delimited JSON over stdin/stdout
│   │   ├── streamable_http.rs    # Streamable HTTP: POST + Mcp-Session-Id (MCP 2025-03-26)
│   │   └── sse.rs               # Legacy SSE: GET stream + POST messages
│   │
│   ├── gateway/                   # [Phase 1 - FULL]
│   │   ├── mod.rs
│   │   ├── proxy.rs              # Upstream request proxying + ID correlation
│   │   ├── router.rs             # Request routing to upstream servers
│   │   └── registry.rs           # Tool registry with namespace collision protection
│   │
│   ├── auth/                      # [Phase 1 - FULL (JWT + scopes)]
│   │   ├── mod.rs
│   │   ├── oauth.rs              # 401 + WWW-Authenticate + PRM discovery chain
│   │   ├── jwt.rs                # JWT validation + JWKS key rotation
│   │   └── scope.rs              # Fine-grained OAuth 2.1 scope enforcement
│   │
│   ├── policy/                    # [Phase 2 - STUB]
│   │   ├── mod.rs
│   │   └── cedar.rs              # Amazon Cedar policy evaluation
│   │
│   ├── session/                   # [Phase 2 - STUB]
│   │   ├── mod.rs
│   │   └── state.rs              # Session state manager (context locking)
│   │
│   ├── guardrail/                 # [Phase 3 - STUB]
│   │   ├── mod.rs
│   │   ├── egress.rs             # Egress payload sanitization
│   │   └── ecpa.rs               # ePCA symbolic constraint framework
│   │
│   ├── telemetry/                 # [Phase 2 metrics live, Phase 2 pipeline STUB]
│   │   ├── mod.rs
│   │   ├── producer.rs           # Redpanda/Kafka async producer
│   │   └── metrics.rs            # Prometheus metrics (request throughput, latency, blocks)
│   │
│   ├── control_plane/             # [Phase 4 - STUB]
│   │   ├── mod.rs
│   │   └── db.rs                  # PostgreSQL tenant + policy config
│   │
│   └── test_server/               # [Phase 1 - FULL]
│       ├── mod.rs
│       └── echo.rs               # Built-in echo MCP server for testing
├── tests/
│   ├── common/
│   │   └── mod.rs                # Test utilities
│   ├── jsonrpc_test.rs          # JSON-RPC parser tests
│   ├── schema_test.rs           # Schema validation tests
│   ├── auth_test.rs             # Auth middleware tests
│   ├── registry_test.rs         # Tool registry + collision tests
│   ├── transport_stdio_test.rs   # Stdio transport integration tests
│   └── transport_http_test.rs   # Streamable HTTP + SSE transport tests
```

---

## Phase 1: Core Routing & Validation (FULL Implementation)

### Module 1: `src/error.rs` — Unified Error Types
- `McpError` enum with JSON-RPC error codes (-32700 to -32603) and custom MCP codes (-32000 to -32099)
- `ProtocolError`, `AuthError`, `ValidationError`, `RegistryError` subtypes
- Auto-conversion to `jsonrpsee` Error types and axum HTTP responses

### Module 2: `src/config.rs` — Configuration
- Typed config struct deserialized from TOML
- Server settings (bind address, log level, CORS origins)
- Auth settings (JWT secret/JWKS URL, issuer, PRM URL, required scopes)
- Proxy settings (upstream server definitions with transport type and URL)
- Registry settings (allowed namespace prefixes)

### Module 3: `src/protocol/jsonrpc.rs` — JSON-RPC 2.0 Parser
- Deserialize raw JSON into typed enum:
  - `JsonRpcMessage::Request { id, method, params }` — id is String or Integer
  - `JsonRpcMessage::Notification { method, params }` — no id field
  - `JsonRpcMessage::Response { id, result }` — success
  - `JsonRpcMessage::Error { id, error }` — error with code/message/data
  - `JsonRpcMessage::Batch(Vec<JsonRpcMessage>)` — batch mode
- Validate: `jsonrpc` field MUST be `"2.0"`, `id` MUST be present for requests, MUST NOT be present for notifications
- Serialize back to JSON with proper ID correlation
- Reject malformed messages with `-32700` (Parse error) or `-32600` (Invalid Request)

### Module 4: `src/protocol/message.rs` — MCP Message Types
- Typed structs for each MCP method:
  - `InitializeParams`, `InitializeResult` (protocol version + capabilities negotiation)
  - `ToolsListParams`, `ToolsListResult` (tool discovery)
  - `ToolsCallParams`, `ToolsCallResult` (tool invocation with name + arguments + content)
  - `ResourcesListParams`, `ResourcesListResult`
  - `PromptsListParams`, `PromptsListResult`
  - `PingParams`, `PongResult`
- `ServerCapabilities`, `ClientCapabilities` structs
- `_meta` with `progressToken` support
- `Tool` struct: name, description, inputSchema, annotations (readOnlyHint, destructiveHint, etc.)

### Module 5: `src/protocol/schema.rs` — JSON Schema 2020-12 Validation
- Use `jsonschema` crate (v0.22) configured for Draft 2020-12
- `SchemaValidator` struct: compile schemas once, validate many
- Validate `tools/call` arguments against the tool's registered `inputSchema`
- Detect unsupported `$schema` dialect declarations → return MCP error with descriptive message
- Graceful fallback: attempt validation with best-effort on unknown dialects

### Module 6: `src/transport/stdio.rs` — Stdio Transport
- Spawn as child process or run as parent
- Read newline-delimited JSON from stdin (tokio `BufReader<Stdin>`)
- Write JSON lines to stdout
- stderr reserved for diagnostic logging (tracing-subscriber)
- Handle batch messages (JSON arrays on a single line)
- Empty lines ignored per spec

### Module 7: `src/transport/streamable_http.rs` — Streamable HTTP Transport
- Axum POST handler at `/mcp` endpoint
- Parse JSON-RPC message from request body
- Generate `Mcp-Session-Id` UUID on first request (initialize), store in session map
- Return session ID in response header; client must echo it back
- 202 Accepted for notifications (no response body needed)
- 200 OK for requests with JSON-RPC response body
- 404 for unknown/terminated sessions

### Module 8: `src/transport/sse.rs` — Legacy SSE Transport
- Axum GET handler `/sse` → SSE event stream (server→client: responses, notifications)
- Axum POST handler `/messages` → client→server messages
- Channel-based: POST writes to mpsc channel, GET reads and sends as SSE events
- Client ID via query param or cookie for correlation
- Automatic reconnection support with `Last-Event-ID`

### Module 9: `src/gateway/registry.rs` — Tool Registry with Namespace Isolation
- `ToolRegistry` struct: in-memory HashMap of registered tools
- Tool name format: `prefix:name` where prefix follows reverse DNS (e.g., `com.github.issues:create`)
- Strict validation:
  - Prefix must contain at least one dot, only lowercase alphanumeric + hyphens
  - Name: 1-128 chars, lowercase alphanumeric + underscores
  - No ambiguous underscore-only concatenation (reject `service_tool` format)
- Collision detection: reject if `prefix:name` already registered
- `register_tool()`, `lookup_tool()`, `list_tools()` methods
- Per-upstream-server tool aggregation with server-of-origin tracking

### Module 10: `src/gateway/router.rs` — Request Routing
- Route incoming MCP method requests to appropriate handler
- `initialize` → capability negotiation + session creation
- `tools/list` → aggregate from registry
- `tools/call` → validate against schema → route to upstream
- `ping` → pong
- Unknown methods → `-32601` Method not found

### Module 11: `src/gateway/proxy.rs` — Upstream Proxy
- `UpstreamClient` trait for connecting to backend MCP servers
- HTTP client implementation for Streamable HTTP backends
- Request forwarding with ID correlation (preserve original request ID)
- Response passthrough after validation
- Connection pooling (tokio `Semaphore` for concurrency control)
- Timeout management per-request
- Built-in echo server integration

### Module 12: `src/auth/jwt.rs` — JWT Validation
- Extract Bearer token from `Authorization` header
- Validate signature using configured secret or JWKS endpoint
- Check issuer, audience, expiration, not-before claims
- `DecodingKey` caching for JWKS (periodic refresh)

### Module 13: `src/auth/oauth.rs` — OAuth 2.1 Interceptor
- On unauthenticated request → 401 Unauthorized response
- `WWW-Authenticate: Bearer resource_metadata="/.well-known/oauth-protected-resource"` header
- Serve PRM document at `/.well-known/oauth-protected-resource` endpoint
- PRM links to authorization server metadata (OIDC discovery)
- Serve `/.well-known/oauth-authorization-server` metadata document

### Module 14: `src/auth/scope.rs` — Scope Enforcement
- Parse OAuth 2.1 scopes from validated JWT
- Map scopes to permitted MCP methods: `mcp:tools:read` → `tools/list`, `mcp:tools:call` → `tools/call`
- Per-tool scope granularity: `mcp:tools:call:{prefix}` → restrict to specific tool prefixes
- Reject unauthorized method/tool calls with `-32002` custom error

### Module 15: `src/test_server/echo.rs` — Echo Test Server
- Implements MCP server protocol (initialize handshake, capability negotiation)
- Registers echo tool: returns its own input arguments as text content
- Registers a few sample tools (add, search, get_time) with proper schemas
- Supports both stdio and HTTP transport
- Configured as default upstream in dev mode

### Module 16: `src/telemetry/metrics.rs` — Prometheus Metrics (Phase 1 subset)
- `mcp_requests_total` (counter, labels: method, transport, status)
- `mcp_request_duration_seconds` (histogram, labels: method)
- `mcp_active_sessions` (gauge)
- `mcp_auth_failures_total` (counter, labels: reason)
- `mcp_validation_failures_total` (counter, labels: reason)
- Exposed at `/metrics` endpoint

### Module 17: `src/main.rs` — Server Bootstrap
- Parse config from `config/default.toml` + env vars + CLI args
- Initialize tracing with structured JSON output
- Build Cedar Authorizer (loaded from policy files)
- Start transport listeners (stdio or HTTP based on config)
- Mount metrics endpoint
- Graceful shutdown with tokio signal handling

---

## Phase 2-4: Module Stubs

Each stub module will contain:
- `mod.rs` with `#[cfg(feature = "...")]` gates
- Public trait/type definitions for the module's contract
- `todo!()` or `unimplemented!()` placeholders with doc comments describing the full implementation
- This allows the project to compile and run Phase 1 while providing a clear roadmap

### Phase 2 Stubs (authz + telemetry)
- `policy/cedar.rs`: `CedarAuthorizer` trait + `evaluate()` method signature
- `session/state.rs`: `SessionManager` trait with `lock_context()`, `check_locked()`, `log_access()`
- `telemetry/producer.rs`: `EventProducer` trait with `publish_audit_event()`

### Phase 3 Stubs (guardrails)
- `guardrail/egress.rs`: `EgressInspector` trait with `sanitize_response()`
- `guardrail/ecpa.rs`: `EcpaConstraint` trait with `evaluate_constraints()`

### Phase 4 Stubs (enterprise)
- `control_plane/db.rs`: `ControlPlane` trait with `load_policies()`, `load_tenants()`

---

## Cargo.toml Key Dependencies

```toml
[dependencies]
# Async runtime
tokio = { version = "1", features = ["full"] }

# Web framework + SSE
axum = { version = "0.8", features = ["json"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["cors", "trace", "auth"] }

# JSON-RPC 2.0
jsonrpsee = { version = "0.26", features = ["server", "client", "macros"] }

# JSON Schema 2020-12
jsonschema = "0.22"
schemars = "1.0"

# JWT + OAuth
jsonwebtoken = "11"
reqwest = { version = "0.12", features = ["json"] }

# Cedar policy engine (loaded but minimal use in Phase 1)
cedar-policy = "4.11"

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"

# Observability
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
metrics = "0.24"
metrics-exporter-prometheus = "0.18"

# Utilities
uuid = { version = "1", features = ["v4", "serde"] }
thiserror = "2"
chrono = { version = "0.4", features = ["serde"] }
```

---

## Helm Chart (Scaffold)

- `Chart.yaml`: API version v2, appVersion from Cargo.toml
- `values.yaml`: replicaCount, image, service (port 8080), resources, configMap
- `deployment.yaml`: Container with config volume mount, liveness/readiness probes on `/healthz`
- `service.yaml`: ClusterIP Service on port 8080 + metrics port 9090
- `configmap.yaml`: Inject `default.toml` as mounted config
- `_helpers.tpl`: Standard Helm helper templates (labels, names)

---

## Docker (Scaffold)

- Multi-stage build: `rust:1.89` builder → `debian:bookworm-slim` runtime
- Copy binary, config, policies into runtime image
- Non-root user for security
- Health check on `/healthz`

---

## Testing Strategy (Phase 1)

- **Unit tests** (in each module): JSON-RPC parsing, schema validation, tool registry collision detection, scope parsing
- **Integration tests** (in `tests/`): Full request lifecycle through each transport, auth flow with test JWTs, proxy to echo server
- **Contract tests**: Verify JSON-RPC messages match MCP spec schema

---

## Execution Order

1. `cargo init` → set up project
2. Write `Cargo.toml` with all dependencies
3. Write `src/error.rs`, `src/config.rs`, `src/lib.rs`
4. Write `src/protocol/jsonrpc.rs` + tests
5. Write `src/protocol/message.rs` + tests
6. Write `src/protocol/schema.rs` + tests
7. Write `src/auth/jwt.rs`, `src/auth/oauth.rs`, `src/auth/scope.rs` + tests
8. Write `src/gateway/registry.rs` + tests
9. Write `src/test_server/echo.rs`
10. Write `src/gateway/proxy.rs`, `src/gateway/router.rs` + tests
11. Write `src/transport/stdio.rs`, `src/transport/streamable_http.rs`, `src/transport/sse.rs` + tests
12. Write `src/telemetry/metrics.rs`
13. Write `src/main.rs` — bootstrap, wire everything together
14. Write Phase 2-4 module stubs with trait contracts
15. Write Helm chart and Dockerfile scaffolds
16. Write `config/default.toml`
17. Write `README.md`
18. `cargo build` + `cargo test` — verify everything compiles and passes
