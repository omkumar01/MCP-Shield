//! Built-in echo test MCP server.
//!
//! Provides a simple MCP server for development and integration testing
//! without requiring external dependencies. The echo server implements the
//! MCP initialize handshake and registers sample tools.

use crate::protocol::message::*;
use serde_json::{json, Value};

/// The echo test server.
///
/// Implements a minimal MCP server that:
/// - Responds to `initialize` with capability negotiation
/// - Lists sample tools (echo, add, get_time)
/// - Handles `tools/call` for each registered tool
/// - Responds to `ping`
pub struct EchoServer;

impl EchoServer {
    /// Return the server's capabilities for the initialize response.
    pub fn capabilities() -> ServerCapabilities {
        ServerCapabilities {
            tools: Some(ToolsCapability {
                list_changed: Some(true),
            }),
            resources: Some(ResourcesCapability {
                list_changed: Some(true),
                subscribe: Some(false),
            }),
            prompts: Some(PromptsCapability {
                list_changed: Some(true),
            }),
            completions: Some(json!({})),
            logging: Some(json!({})),
            experimental: None,
        }
    }

    /// Return the server info for the initialize response.
    pub fn server_info() -> Implementation {
        Implementation {
            name: "mcp-shield-echo".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Return the list of tools provided by the echo server.
    pub fn list_tools() -> Vec<Tool> {
        vec![
            Tool {
                name: "com.echo:echo".to_string(),
                description: Some("Echoes back the input message as text.".to_string()),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "message": {
                            "type": "string",
                            "description": "The message to echo back"
                        }
                    },
                    "required": ["message"]
                }),
                annotations: Some(ToolAnnotations {
                    read_only_hint: Some(true),
                    destructive_hint: Some(false),
                    idempotent_hint: Some(true),
                    open_world_hint: Some(false),
                    title: Some("Echo".to_string()),
                }),
            },
            Tool {
                name: "com.echo:add".to_string(),
                description: Some("Adds two integers and returns the sum.".to_string()),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "a": {
                            "type": "integer",
                            "description": "First addend"
                        },
                        "b": {
                            "type": "integer",
                            "description": "Second addend"
                        }
                    },
                    "required": ["a", "b"]
                }),
                annotations: Some(ToolAnnotations {
                    read_only_hint: Some(true),
                    destructive_hint: Some(false),
                    idempotent_hint: Some(true),
                    open_world_hint: Some(false),
                    title: Some("Add".to_string()),
                }),
            },
            Tool {
                name: "com.echo:get_time".to_string(),
                description: Some("Returns the current server time in RFC 3339 format.".to_string()),
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }),
                annotations: Some(ToolAnnotations {
                    read_only_hint: Some(true),
                    destructive_hint: Some(false),
                    idempotent_hint: Some(false),
                    open_world_hint: Some(false),
                    title: Some("Get Time".to_string()),
                }),
            },
            Tool {
                name: "com.echo:uppercase".to_string(),
                description: Some("Converts a string to uppercase.".to_string()),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "The text to convert"
                        }
                    },
                    "required": ["text"]
                }),
                annotations: Some(ToolAnnotations {
                    read_only_hint: Some(true),
                    destructive_hint: Some(false),
                    idempotent_hint: Some(true),
                    open_world_hint: Some(false),
                    title: Some("Uppercase".to_string()),
                }),
            },
        ]
    }

    /// Handle a tool call.
    ///
    /// Returns the tool result as JSON, or an error if the tool is unknown.
    pub fn handle_tool_call(name: &str, arguments: &Value) -> Result<Value, String> {
        match name {
            "com.echo:echo" => {
                let message = arguments
                    .get("message")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required argument 'message'")?;
                Ok(json!({
                    "content": [
                        {"type": "text", "text": format!("Echo: {}", message)}
                    ]
                }))
            }
            "com.echo:add" => {
                let a = arguments
                    .get("a")
                    .and_then(|v| v.as_i64())
                    .ok_or("Missing required argument 'a'")?;
                let b = arguments
                    .get("b")
                    .and_then(|v| v.as_i64())
                    .ok_or("Missing required argument 'b'")?;
                Ok(json!({
                    "content": [
                        {"type": "text", "text": format!("{} + {} = {}", a, b, a + b)}
                    ]
                }))
            }
            "com.echo:get_time" => {
                Ok(json!({
                    "content": [
                        {"type": "text", "text": chrono::Utc::now().to_rfc3339()}
                    ]
                }))
            }
            "com.echo:uppercase" => {
                let text = arguments
                    .get("text")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing required argument 'text'")?;
                Ok(json!({
                    "content": [
                        {"type": "text", "text": text.to_uppercase()}
                    ]
                }))
            }
            _ => Err(format!("Unknown tool: {}", name)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_tools_count() {
        let tools = EchoServer::list_tools();
        assert_eq!(tools.len(), 4);
    }

    #[test]
    fn test_tool_names_qualified() {
        let tools = EchoServer::list_tools();
        for tool in &tools {
            assert!(
                tool.name.contains(':'),
                "Tool name '{}' should use qualified format",
                tool.name
            );
        }
    }

    #[test]
    fn test_echo_tool_call() {
        let args = json!({"message": "hello"});
        let result = EchoServer::handle_tool_call("com.echo:echo", &args).unwrap();
        assert_eq!(
            result["content"][0]["text"],
            "Echo: hello"
        );
    }

    #[test]
    fn test_add_tool_call() {
        let args = json!({"a": 5, "b": 3});
        let result = EchoServer::handle_tool_call("com.echo:add", &args).unwrap();
        assert_eq!(
            result["content"][0]["text"],
            "5 + 3 = 8"
        );
    }

    #[test]
    fn test_unknown_tool_returns_error() {
        let result = EchoServer::handle_tool_call("com.echo:unknown", &json!({}));
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_argument() {
        let result = EchoServer::handle_tool_call("com.echo:echo", &json!({}));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("message"));
    }

    #[test]
    fn test_uppercase_tool() {
        let args = json!({"text": "hello world"});
        let result = EchoServer::handle_tool_call("com.echo:uppercase", &args).unwrap();
        assert_eq!(
            result["content"][0]["text"],
            "HELLO WORLD"
        );
    }
}
