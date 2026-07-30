//! Gateway layer for MCP-Shield.
//!
//! Provides the tool registry (with namespace collision protection),
//! upstream proxy, and request router that orchestrates the gateway pipeline.

pub mod proxy;
pub mod registry;
pub mod router;

pub use proxy::{UpstreamProxy, UpstreamServer, UpstreamTransport};
pub use registry::{RegisteredTool, ToolRegistry};
pub use router::McpRouter;
