//! Transport layer for MCP-Shield.
//!
//! Provides three transports per the MCP specification:
//! - Stdio (newline-delimited JSON over stdin/stdout)
//! - Streamable HTTP (POST + Mcp-Session-Id, MCP 2025-03-26)
//! - Legacy SSE (GET stream + POST messages)

pub mod sse;
pub mod stdio;
pub mod streamable_http;

pub use sse::SseState;
pub use stdio::StdioTransport;
pub use streamable_http::StreamableHttpState;
