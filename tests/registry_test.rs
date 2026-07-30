//! Integration tests for the tool registry namespace collision protection.

use mcp_shield::gateway::registry::ToolRegistry;
use mcp_shield::protocol::message::Tool;
use serde_json::json;

fn make_tool(name: &str) -> Tool {
    Tool {
        name: name.to_string(),
        description: Some(format!("{} tool", name)),
        input_schema: json!({"type": "object"}),
        annotations: None,
    }
}

#[tokio::test]
async fn test_register_and_list_tools() {
    let registry = ToolRegistry::new();

    registry
        .register_tool(make_tool("com.example:echo"), "server-1")
        .await
        .unwrap();
    registry
        .register_tool(make_tool("com.example:search"), "server-1")
        .await
        .unwrap();
    registry
        .register_tool(make_tool("io.github:create_pr"), "server-2")
        .await
        .unwrap();

    let tools = registry.list_tools().await;
    assert_eq!(tools.len(), 3);
}

#[tokio::test]
async fn test_collision_between_servers() {
    let registry = ToolRegistry::new();

    // Server 1 registers a tool
    registry
        .register_tool(make_tool("com.example:echo"), "server-1")
        .await
        .unwrap();

    // Server 2 tries to register the same tool name
    let result = registry
        .register_tool(make_tool("com.example:echo"), "server-2")
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("collision"));
    assert!(err.contains("server-1"));
    assert!(err.contains("server-2"));

    // Only the original tool should be registered
    assert_eq!(registry.count().await, 1);
}

#[tokio::test]
async fn test_reject_ambiguous_underscore_format() {
    let registry = ToolRegistry::new();

    // Attempt to look up a tool using ambiguous underscore format
    let result = registry.lookup_any("github_search_issues").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Ambiguous"));
}

#[tokio::test]
async fn test_per_server_tool_isolation() {
    let registry = ToolRegistry::new();

    registry
        .register_tool(make_tool("com.example:echo"), "server-1")
        .await
        .unwrap();
    registry
        .register_tool(make_tool("com.github:create_pr"), "server-2")
        .await
        .unwrap();

    // List by server
    let server1_tools = registry.list_tools_by_server("server-1").await;
    assert_eq!(server1_tools.len(), 1);
    assert_eq!(server1_tools[0].name, "com.example:echo");

    let server2_tools = registry.list_tools_by_server("server-2").await;
    assert_eq!(server2_tools.len(), 1);
    assert_eq!(server2_tools[0].name, "com.github:create_pr");
}

#[tokio::test]
async fn test_unregister_server_clears_tools() {
    let registry = ToolRegistry::new();

    registry
        .register_tool(make_tool("com.example:echo"), "server-1")
        .await
        .unwrap();
    registry
        .register_tool(make_tool("com.example:search"), "server-1")
        .await
        .unwrap();

    assert_eq!(registry.count().await, 2);

    let removed = registry.unregister_server("server-1").await;
    assert_eq!(removed, 2);
    assert_eq!(registry.count().await, 0);
}

#[tokio::test]
async fn test_prefix_format_enforcement() {
    let registry = ToolRegistry::new();

    // Prefix without dot should be rejected
    let result = registry
        .register_tool(make_tool("noprefix:tool"), "server-1")
        .await;
    assert!(result.is_err());

    // Valid reverse-DNS prefix should work
    let result = registry
        .register_tool(make_tool("com.example:tool"), "server-1")
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_allowed_prefixes_filter() {
    let registry = ToolRegistry::with_config(true, vec!["com.allowed".to_string()]);

    // Allowed prefix works
    assert!(
        registry
            .register_tool(make_tool("com.allowed:tool"), "server-1")
            .await
            .is_ok()
    );

    // Disallowed prefix is rejected
    assert!(
        registry
            .register_tool(make_tool("com.disallowed:tool"), "server-1")
            .await
            .is_err()
    );
}
