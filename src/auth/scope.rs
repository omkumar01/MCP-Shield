//! OAuth 2.1 scope enforcement for MCP-Shield.
//!
//! Maps OAuth 2.1 scopes to permitted MCP methods and tools. Provides
//! fine-grained access control to limit the blast radius of any single
//! token.

use crate::error::McpError;
use crate::protocol::message;
use std::collections::HashSet;

/// Core MCP scope prefixes.
pub const SCOPE_PREFIX: &str = "mcp";

/// Scope for reading tool lists and resources.
pub const SCOPE_TOOLS_READ: &str = "mcp:tools:read";

/// Scope for invoking tools.
pub const SCOPE_TOOLS_CALL: &str = "mcp:tools:call";

/// Scope for reading resources.
pub const SCOPE_RESOURCES_READ: &str = "mcp:resources:read";

/// Scope for reading prompts.
pub const SCOPE_PROMPTS_READ: &str = "mcp:prompts:read";

/// Scope for administrative operations.
pub const SCOPE_ADMIN: &str = "mcp:admin";

/// Scope mapper that checks if a set of OAuth scopes permits a given action.
#[derive(Debug, Clone)]
pub struct ScopeEnforcer {
    /// All scopes granted to the current client.
    granted_scopes: HashSet<String>,

    /// Whether the enforcer is in permissive mode (skip scope checks).
    permissive: bool,
}

impl ScopeEnforcer {
    /// Create a new scope enforcer from a list of granted scope strings.
    pub fn new(scopes: Vec<String>) -> Self {
        Self {
            granted_scopes: scopes.into_iter().collect(),
            permissive: false,
        }
    }

    /// Create a permissive enforcer that allows all actions.
    ///
    /// Used when auth is disabled.
    pub fn permissive() -> Self {
        Self {
            granted_scopes: HashSet::new(),
            permissive: true,
        }
    }

    /// Check if the client has a specific scope.
    pub fn has_scope(&self, scope: &str) -> bool {
        self.permissive || self.granted_scopes.contains(scope)
    }

    /// Check if the client has any scope matching a prefix.
    ///
    /// For example, `mcp:tools:call:*` matches `mcp:tools:call:com.example.echo`.
    pub fn has_scope_prefix(&self, prefix: &str) -> bool {
        if self.permissive {
            return true;
        }
        self.granted_scopes
            .iter()
            .any(|s| s.starts_with(prefix) || s == prefix)
    }

    /// Check if the client is allowed to invoke a specific MCP method.
    pub fn check_method(&self, method: &str) -> Result<(), McpError> {
        if self.permissive {
            return Ok(());
        }

        let required_scope = match method {
            message::METHOD_INITIALIZE | message::METHOD_INITIALIZED | message::METHOD_PING => {
                return Ok(()); // Always allowed
            }
            message::METHOD_TOOLS_LIST
            | message::METHOD_RESOURCES_LIST
            | message::METHOD_PROMPTS_LIST => {
                // Any read scope is sufficient
                if self.has_scope(SCOPE_TOOLS_READ)
                    || self.has_scope(SCOPE_RESOURCES_READ)
                    || self.has_scope(SCOPE_PROMPTS_READ)
                    || self.has_scope(SCOPE_ADMIN)
                {
                    return Ok(());
                }
                return Err(McpError::ScopeDenied(format!(
                    "Scope required to access '{}'. Need one of: {}, {}, {}",
                    method, SCOPE_TOOLS_READ, SCOPE_RESOURCES_READ, SCOPE_PROMPTS_READ
                )));
            }
            message::METHOD_TOOLS_CALL => {
                if self.has_scope(SCOPE_TOOLS_CALL) || self.has_scope(SCOPE_ADMIN) {
                    return Ok(());
                }
                return Err(McpError::ScopeDenied(format!(
                    "Scope '{}' is required to invoke tools",
                    SCOPE_TOOLS_CALL
                )));
            }
            message::METHOD_RESOURCES_READ => {
                if self.has_scope(SCOPE_RESOURCES_READ) || self.has_scope(SCOPE_ADMIN) {
                    return Ok(());
                }
                return Err(McpError::ScopeDenied(format!(
                    "Scope '{}' is required to read resources",
                    SCOPE_RESOURCES_READ
                )));
            }
            message::METHOD_PROMPTS_GET => {
                if self.has_scope(SCOPE_PROMPTS_READ) || self.has_scope(SCOPE_ADMIN) {
                    return Ok(());
                }
                return Err(McpError::ScopeDenied(format!(
                    "Scope '{}' is required to get prompts",
                    SCOPE_PROMPTS_READ
                )));
            }
            message::METHOD_SAMPLING_CREATE => {
                // Sampling requires a special scope
                if self.has_scope("mcp:sampling") || self.has_scope(SCOPE_ADMIN) {
                    return Ok(());
                }
                return Err(McpError::ScopeDenied(
                    "Scope 'mcp:sampling' is required for sampling".to_string(),
                ));
            }
            message::METHOD_SHUTDOWN => {
                // Shutdown requires admin scope
                if self.has_scope(SCOPE_ADMIN) {
                    return Ok(());
                }
                return Err(McpError::ScopeDenied(format!(
                    "Scope '{}' is required to shutdown the gateway",
                    SCOPE_ADMIN
                )));
            }
            // Notification methods are always allowed (server → client)
            _ => return Ok(()),
        };
    }

    /// Check if the client is allowed to call a specific tool.
    ///
    /// Supports per-tool-prefix scope granularity:
    /// - `mcp:tools:call` → allow all tool calls
    /// - `mcp:tools:call:com.example` → allow only tools with prefix `com.example`
    /// - `mcp:tools:call:com.example.echo` → allow only the specific tool
    pub fn check_tool_access(&self, tool_name: &str) -> Result<(), McpError> {
        if self.permissive {
            return Ok(());
        }

        // Check for admin scope (full access)
        if self.has_scope(SCOPE_ADMIN) {
            return Ok(());
        }

        // Check for global tools:call scope
        if self.has_scope(SCOPE_TOOLS_CALL) {
            return Ok(());
        }

        // Check for prefix-specific scopes
        // Parse the tool name as `prefix:name`
        let parts: Vec<&str> = tool_name.splitn(2, ':').collect();
        if parts.len() == 2 {
            let prefix = parts[0];
            if self.has_scope(&format!("{}:{}", SCOPE_TOOLS_CALL, prefix)) {
                return Ok(());
            }
            // Check for exact tool scope
            if self.has_scope(&format!("{}:{}", SCOPE_TOOLS_CALL, tool_name)) {
                return Ok(());
            }
        }

        Err(McpError::ScopeDenied(format!(
            "Access to tool '{}' denied. Scope '{}' or '{}' is required",
            tool_name, SCOPE_TOOLS_CALL, SCOPE_ADMIN
        )))
    }

    /// Get the list of granted scopes.
    pub fn granted_scopes(&self) -> &HashSet<String> {
        &self.granted_scopes
    }

    /// Add a scope to the enforcer.
    pub fn add_scope(&mut self, scope: impl Into<String>) {
        self.granted_scopes.insert(scope.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permissive_allows_all() {
        let enforcer = ScopeEnforcer::permissive();
        assert!(enforcer.check_method("tools/call").is_ok());
        assert!(enforcer.check_method("shutdown").is_ok());
        assert!(enforcer.check_tool_access("com.example.echo").is_ok());
    }

    #[test]
    fn test_tools_read_allows_list() {
        let enforcer = ScopeEnforcer::new(vec![SCOPE_TOOLS_READ.to_string()]);
        assert!(enforcer.check_method("tools/list").is_ok());
        assert!(enforcer.check_method("resources/list").is_ok());
    }

    #[test]
    fn test_tools_call_denied_without_scope() {
        let enforcer = ScopeEnforcer::new(vec![SCOPE_TOOLS_READ.to_string()]);
        assert!(enforcer.check_method("tools/call").is_err());
    }

    #[test]
    fn test_tools_call_allowed_with_scope() {
        let enforcer = ScopeEnforcer::new(vec![SCOPE_TOOLS_CALL.to_string()]);
        assert!(enforcer.check_method("tools/call").is_ok());
    }

    #[test]
    fn test_shutdown_requires_admin() {
        let enforcer = ScopeEnforcer::new(vec![SCOPE_TOOLS_CALL.to_string()]);
        assert!(enforcer.check_method("shutdown").is_err());

        let admin = ScopeEnforcer::new(vec![SCOPE_ADMIN.to_string()]);
        assert!(admin.check_method("shutdown").is_ok());
    }

    #[test]
    fn test_initialize_always_allowed() {
        let enforcer = ScopeEnforcer::new(vec![]);
        assert!(enforcer.check_method("initialize").is_ok());
        assert!(enforcer.check_method("notifications/initialized").is_ok());
        assert!(enforcer.check_method("ping").is_ok());
    }

    #[test]
    fn test_per_tool_prefix_scope() {
        let enforcer = ScopeEnforcer::new(vec![format!("{}:com.example", SCOPE_TOOLS_CALL)]);
        assert!(enforcer.check_tool_access("com.example:echo").is_ok());
        assert!(enforcer.check_method("tools/call").is_err()); // global scope not granted
        assert!(enforcer.check_tool_access("com.other:search").is_err()); // different prefix
    }

    #[test]
    fn test_exact_tool_scope() {
        let enforcer = ScopeEnforcer::new(vec![format!("{}:com.example:echo", SCOPE_TOOLS_CALL)]);
        assert!(enforcer.check_tool_access("com.example:echo").is_ok());
        assert!(enforcer.check_tool_access("com.example:delete").is_err());
    }

    #[test]
    fn test_has_scope_prefix() {
        let enforcer = ScopeEnforcer::new(vec!["mcp:tools:call:com.example".to_string()]);
        assert!(enforcer.has_scope_prefix("mcp:tools:call:com.example"));
        assert!(enforcer.has_scope_prefix("mcp:tools:call"));
        assert!(!enforcer.has_scope_prefix("mcp:resources"));
    }
}
