//! MCP-Shield: A Layer 7 protocol-aware security gateway for MCP.
//!
//! MCP-Shield intercepts, validates, and authorizes all traffic between
//! MCP Hosts (AI applications) and MCP Servers. It enforces JSON-RPC 2.0
//! protocol compliance, JSON Schema 2020-12 validation, OAuth 2.1
//! authentication, and fine-grained scope-based authorization.
//!
//! ## Architecture
//!
//! ```text
//! MCP Host ──► [Transport] ──► [Protocol Parser] ──► [Auth + Scope]
//!                                                          │
//! MCP Server ◄── [Upstream Proxy] ◄── [Router] ◄── [Schema Validator]
//! ```
//!
//! ## Quick Start
//!
//! ```no_run
//! use mcp_shield::config::Config;
//! use mcp_shield::telemetry::McpMetrics;
//!
//! let config = Config::default();
//! let metrics = McpMetrics::new();
//! ```

pub mod auth;
pub mod config;
pub mod control_plane;
pub mod error;
pub mod gateway;
pub mod guardrail;
pub mod policy;
pub mod protocol;
pub mod session;
pub mod telemetry;
pub mod test_server;
pub mod transport;

// Re-export the most commonly used types
pub use auth::{JwtValidator, ScopeEnforcer};
pub use config::Config;
pub use error::McpError;
pub use gateway::{McpRouter, ToolRegistry, UpstreamProxy};
pub use protocol::{JsonRpcMessage, SchemaValidator};
pub use telemetry::McpMetrics;
pub use test_server::EchoServer;

/// The version of MCP-Shield.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The MCP protocol version supported by this gateway.
pub const MCP_PROTOCOL_VERSION: &str = "2025-03-26";
