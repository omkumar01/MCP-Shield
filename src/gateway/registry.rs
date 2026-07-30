//! Tool registry with strict namespace isolation.
//!
//! Prevents tool name collisions by enforcing reverse-DNS prefix naming
//! and rejecting ambiguous underscore-only concatenation. This neutralizes
//! a class of attacks where malicious servers overwrite legitimate tools.

use crate::error::{McpError, REGISTRY_COLLISION};
use crate::protocol::message::Tool;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Maximum tool name length per MCP specification.
pub const MAX_TOOL_NAME_LENGTH: usize = 128;

/// Minimum tool name length.
pub const MIN_TOOL_NAME_LENGTH: usize = 1;

/// A registered tool entry with its origin metadata.
#[derive(Debug, Clone)]
pub struct RegisteredTool {
    /// The full qualified tool name (prefix:name).
    pub qualified_name: String,

    /// The namespace prefix (e.g., "com.github").
    pub prefix: String,

    /// The tool name within the prefix (e.g., "create_pr").
    pub name: String,

    /// The tool definition (name, description, inputSchema, annotations).
    pub tool: Tool,

    /// The upstream server that provides this tool.
    pub server_id: String,

    /// Registration timestamp.
    pub registered_at: chrono::DateTime<chrono::Utc>,
}

/// Thread-safe tool registry with namespace collision protection.
#[derive(Debug, Clone)]
pub struct ToolRegistry {
    inner: Arc<RwLock<ToolRegistryInner>>,
}

#[derive(Debug)]
struct ToolRegistryInner {
    /// All registered tools keyed by qualified name.
    tools: HashMap<String, RegisteredTool>,

    /// Index by server ID for quick lookup per upstream.
    by_server: HashMap<String, Vec<String>>,

    /// Whether to enforce strict reverse-DNS prefix format.
    enforce_prefix_format: bool,

    /// Allowed namespace prefixes (empty = allow all).
    allowed_prefixes: Vec<String>,
}

impl ToolRegistry {
    /// Create a new empty tool registry.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(ToolRegistryInner {
                tools: HashMap::new(),
                by_server: HashMap::new(),
                enforce_prefix_format: true,
                allowed_prefixes: Vec::new(),
            })),
        }
    }

    /// Create a registry with configuration options.
    pub fn with_config(enforce_prefix_format: bool, allowed_prefixes: Vec<String>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(ToolRegistryInner {
                tools: HashMap::new(),
                by_server: HashMap::new(),
                enforce_prefix_format,
                allowed_prefixes,
            })),
        }
    }

    /// Register a tool from an upstream server.
    ///
    /// Validates the tool name format and checks for collisions before registering.
    pub async fn register_tool(
        &self,
        tool: Tool,
        server_id: &str,
    ) -> Result<(), McpError> {
        let mut inner = self.inner.write().await;
        let (prefix, name) = parse_tool_name(&tool.name, inner.enforce_prefix_format)?;

        // Check allowed prefixes
        if !inner.allowed_prefixes.is_empty() && !inner.allowed_prefixes.contains(&prefix) {
            return Err(McpError::RegistryCollision(format!(
                "Tool prefix '{}' is not in the allowed prefixes list",
                prefix
            )));
        }

        let qualified_name = format!("{}:{}", prefix, name);

        // Check for collisions
        if inner.tools.contains_key(&qualified_name) {
            let existing = &inner.tools[&qualified_name];
            return Err(McpError::RegistryCollision(format!(
                "Tool name collision: '{}' is already registered by server '{}'. \
                 Rejecting registration from server '{}'. \
                 This prevents potential tool hijacking attacks.",
                qualified_name, existing.server_id, server_id
            )));
        }

        // Register the tool
        let registered = RegisteredTool {
            qualified_name: qualified_name.clone(),
            prefix: prefix.clone(),
            name: name.clone(),
            tool,
            server_id: server_id.to_string(),
            registered_at: chrono::Utc::now(),
        };

        inner
            .by_server
            .entry(server_id.to_string())
            .or_default()
            .push(qualified_name.clone());

        inner.tools.insert(qualified_name.clone(), registered);

        tracing::info!(
            tool = %qualified_name,
            prefix = %prefix,
            server = %server_id,
            "Registered tool"
        );

        Ok(())
    }

    /// Look up a tool by its qualified name.
    pub async fn lookup(&self, qualified_name: &str) -> Option<RegisteredTool> {
        let inner = self.inner.read().await;
        inner.tools.get(qualified_name).cloned()
    }

    /// Look up a tool by any name format (qualified, bare, or legacy underscore).
    ///
    /// If the name contains a colon, treats it as prefix:name.
    /// If the name contains an underscore, attempts to parse as prefix_name (legacy format)
    /// but only matches if there's an exact qualified name match.
    pub async fn lookup_any(&self, name: &str) -> Result<Option<RegisteredTool>, McpError> {
        // First try direct qualified lookup
        if let Some(tool) = self.lookup(name).await {
            return Ok(Some(tool));
        }

        // Reject underscore-only concatenation format (ambiguous, security risk)
        if name.contains('_') && !name.contains(':') {
            return Err(McpError::RegistryCollision(format!(
                "Ambiguous tool name '{}' uses underscore concatenation without a colon separator. \
                 This format is not allowed as it can be exploited for tool name collision attacks. \
                 Use the qualified format 'prefix:name' instead (e.g., 'com.example:tool_name').",
                name
            )));
        }

        Ok(None)
    }

    /// List all registered tools.
    pub async fn list_tools(&self) -> Vec<Tool> {
        let inner = self.inner.read().await;
        inner
            .tools
            .values()
            .map(|r| r.tool.clone())
            .collect()
    }

    /// List all registered tools from a specific upstream server.
    pub async fn list_tools_by_server(&self, server_id: &str) -> Vec<Tool> {
        let inner = self.inner.read().await;
        inner
            .by_server
            .get(server_id)
            .map(|names| {
                names
                    .iter()
                    .filter_map(|name| inner.tools.get(name).map(|r| r.tool.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Remove all tools registered by a specific server.
    ///
    /// Called when an upstream server disconnects or is deregistered.
    pub async fn unregister_server(&self, server_id: &str) -> usize {
        let mut inner = self.inner.write().await;
        if let Some(names) = inner.by_server.remove(server_id) {
            let count = names.len();
            for name in &names {
                inner.tools.remove(name);
            }
            tracing::info!(server = %server_id, count, "Unregistered all tools from server");
            count
        } else {
            0
        }
    }

    /// Get the total number of registered tools.
    pub async fn count(&self) -> usize {
        self.inner.read().await.tools.len()
    }

    /// Clear all registered tools.
    pub async fn clear(&self) {
        let mut inner = self.inner.write().await;
        inner.tools.clear();
        inner.by_server.clear();
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse a tool name into (prefix, name) components.
///
/// Enforces strict reverse-DNS format for prefixes:
/// - Prefix must contain at least one dot (e.g., "com.example")
/// - Only lowercase alphanumeric characters and hyphens allowed
/// - Name: 1-128 chars, lowercase alphanumeric and underscores
fn parse_tool_name(
    raw_name: &str,
    enforce_prefix: bool,
) -> Result<(String, String), McpError> {
    let parts: Vec<&str> = raw_name.splitn(2, ':').collect();

    if parts.len() != 2 {
        return Err(McpError::RegistryCollision(format!(
            "Tool name '{}' must use qualified format 'prefix:name' (e.g., 'com.example:search'). \
             Underscore-only concatenation is not allowed.",
            raw_name
        )));
    }

    let prefix = parts[0].trim();
    let name = parts[1].trim();

    if prefix.is_empty() {
        return Err(McpError::RegistryCollision(
            "Tool prefix cannot be empty".to_string(),
        ));
    }

    if name.is_empty() {
        return Err(McpError::RegistryCollision(
            "Tool name within prefix cannot be empty".to_string(),
        ));
    }

    if enforce_prefix {
        // Prefix must contain at least one dot (reverse DNS)
        if !prefix.contains('.') {
            return Err(McpError::RegistryCollision(format!(
                "Tool prefix '{}' must contain at least one dot (reverse DNS format, e.g., 'com.example')",
                prefix
            )));
        }

        // Prefix must be lowercase alphanumeric + hyphens only
        if !prefix
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
        {
            return Err(McpError::RegistryCollision(format!(
                "Tool prefix '{}' contains invalid characters. Use only lowercase letters, digits, hyphens, and dots.",
                prefix
            )));
        }
    }

    // Name validation
    if name.len() < MIN_TOOL_NAME_LENGTH || name.len() > MAX_TOOL_NAME_LENGTH {
        return Err(McpError::RegistryCollision(format!(
            "Tool name '{}' length must be between {} and {} characters",
            name, MIN_TOOL_NAME_LENGTH, MAX_TOOL_NAME_LENGTH
        )));
    }

    // Name must be lowercase alphanumeric + underscores only
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(McpError::RegistryCollision(format!(
            "Tool name '{}' contains invalid characters. Use only lowercase letters, digits, and underscores.",
            name
        )));
    }

    Ok((prefix.to_string(), name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tool(name: &str) -> Tool {
        Tool {
            name: name.to_string(),
            description: Some(format!("{} tool", name)),
            input_schema: json!({"type": "object"}),
            annotations: None,
        }
    }

    #[tokio::test]
    async fn test_register_and_lookup() {
        let registry = ToolRegistry::new();
        registry
            .register_tool(make_tool("com.example:echo"), "server-1")
            .await
            .unwrap();

        let tool = registry.lookup("com.example:echo").await;
        assert!(tool.is_some());
        let tool = tool.unwrap();
        assert_eq!(tool.prefix, "com.example");
        assert_eq!(tool.name, "echo");
        assert_eq!(tool.server_id, "server-1");
    }

    #[tokio::test]
    async fn test_collision_detection() {
        let registry = ToolRegistry::new();
        registry
            .register_tool(make_tool("com.example:echo"), "server-1")
            .await
            .unwrap();

        let result = registry
            .register_tool(make_tool("com.example:echo"), "server-2")
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("collision"));
        assert!(err.contains("server-1"));
    }

    #[tokio::test]
    async fn test_reject_underscore_concatenation() {
        let registry = ToolRegistry::new();
        let result = registry.lookup_any("github_search_issues").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Ambiguous"));
    }

    #[tokio::test]
    async fn test_reject_no_colon() {
        let registry = ToolRegistry::new();
        let result = registry.register_tool(make_tool("search"), "server-1").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("qualified format"));
    }

    #[tokio::test]
    async fn test_reject_empty_prefix() {
        let registry = ToolRegistry::new();
        let result = registry.register_tool(make_tool(":echo"), "server-1").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_reject_prefix_without_dot() {
        let registry = ToolRegistry::new();
        let result = registry.register_tool(make_tool("example:echo"), "server-1").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("dot"));
    }

    #[tokio::test]
    async fn test_reject_uppercase_in_name() {
        let registry = ToolRegistry::new();
        let result = registry.register_tool(make_tool("com.example:Echo"), "server-1").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalid characters"));
    }

    #[tokio::test]
    async fn test_allowed_prefixes_filter() {
        let registry = ToolRegistry::with_config(true, vec!["com.example".to_string()]);
        registry
            .register_tool(make_tool("com.example:echo"), "server-1")
            .await
            .unwrap();

        let result = registry
            .register_tool(make_tool("com.other:search"), "server-1")
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not in the allowed prefixes"));
    }

    #[tokio::test]
    async fn test_unregister_server() {
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
    async fn test_list_tools() {
        let registry = ToolRegistry::new();
        registry
            .register_tool(make_tool("com.example:echo"), "server-1")
            .await
            .unwrap();
        registry
            .register_tool(make_tool("com.github:create_pr"), "server-2")
            .await
            .unwrap();

        let tools = registry.list_tools().await;
        assert_eq!(tools.len(), 2);
    }

    #[tokio::test]
    async fn test_list_tools_by_server() {
        let registry = ToolRegistry::new();
        registry
            .register_tool(make_tool("com.example:echo"), "server-1")
            .await
            .unwrap();
        registry
            .register_tool(make_tool("com.github:create_pr"), "server-2")
            .await
            .unwrap();

        let tools = registry.list_tools_by_server("server-1").await;
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "com.example:echo");
    }
}
