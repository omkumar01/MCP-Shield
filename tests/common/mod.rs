//! Common test utilities for MCP-Shield integration tests.

use mcp_shield::config::Config;
use mcp_shield::gateway::{ToolRegistry, UpstreamProxy};
use mcp_shield::telemetry::McpMetrics;

/// Create a test configuration with defaults suitable for unit tests.
pub fn test_config() -> Config {
    Config::default()
}

/// Create a test tool registry with echo server tools pre-registered.
pub async fn test_registry() -> ToolRegistry {
    let registry = ToolRegistry::new();
    for tool in mcp_shield::test_server::EchoServer::list_tools() {
        registry.register_tool(tool, "echo").await.unwrap();
    }
    registry
}

/// Create a test upstream proxy with the echo server registered.
pub fn test_proxy() -> UpstreamProxy {
    UpstreamProxy::new(10, 10)
}

/// Create a test metrics recorder.
pub fn test_metrics() -> McpMetrics {
    McpMetrics::new()
}