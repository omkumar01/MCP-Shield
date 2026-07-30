//! Upstream proxy for forwarding validated requests to MCP servers.
//!
//! Supports HTTP-based upstream connections with request ID correlation,
//! connection pooling, and timeout management. Includes a built-in echo
//! test server for development.

use crate::error::{McpError, RequestId};
use crate::protocol::jsonrpc::JsonRpcMessage;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock, Semaphore};

/// An upstream MCP server configuration.
#[derive(Debug, Clone)]
pub struct UpstreamServer {
    /// Unique identifier for this upstream.
    pub id: String,

    /// Transport type.
    pub transport: UpstreamTransport,

    /// URL for HTTP-based transports.
    pub url: Option<String>,

    /// Whether this is the built-in echo test server.
    pub is_echo: bool,
}

/// Upstream transport type.
#[derive(Debug, Clone, PartialEq)]
pub enum UpstreamTransport {
    /// Streamable HTTP (MCP 2025-03-26).
    StreamableHttp,

    /// Legacy SSE + HTTP POST.
    Sse,

    /// Stdio (subprocess).
    Stdio,
}

/// A proxy that forwards requests to upstream MCP servers.
pub struct UpstreamProxy {
    /// Configuration for upstream servers.
    servers: Arc<RwLock<HashMap<String, UpstreamServer>>>,

    /// HTTP client for upstream connections.
    http_client: reqwest::Client,

    /// Semaphore for concurrency control.
    concurrency_limit: Arc<Semaphore>,

    /// Request timeout.
    timeout: Duration,

    /// Built-in echo test server channel.
    echo_tx: Option<mpsc::Sender<EchoRequest>>,
}

/// A request to the echo test server.
#[derive(Debug)]
pub struct EchoRequest {
    /// The original JSON-RPC message.
    pub message: JsonRpcMessage,
    /// Response channel.
    pub reply_tx: oneshot::Sender<Result<JsonRpcMessage, McpError>>,
}

use tokio::sync::oneshot;

impl UpstreamProxy {
    /// Create a new upstream proxy.
    pub fn new(timeout_secs: u64, max_concurrent: usize) -> Self {
        let (echo_tx, _) = mpsc::channel(1000);
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(timeout_secs))
                .build()
                .unwrap_or_else(|_| {
                    reqwest::Client::new()
                }),
            concurrency_limit: Arc::new(Semaphore::new(max_concurrent)),
            timeout: Duration::from_secs(timeout_secs),
            echo_tx: Some(echo_tx),
        }
    }

    /// Register an upstream server.
    pub async fn register_server(&self, server: UpstreamServer) {
        let server_id = server.id.clone();
        let mut servers = self.servers.write().await;
        servers.insert(server_id.clone(), server);
        tracing::info!(server_id = %server_id, "Registered upstream server");
    }

    /// Forward a JSON-RPC request to the appropriate upstream server.
    ///
    /// Returns the upstream response with the original request ID preserved.
    pub async fn forward_request(
        &self,
        server_id: &str,
        message: JsonRpcMessage,
    ) -> Result<JsonRpcMessage, McpError> {
        // Acquire concurrency slot
        let _permit = self
            .concurrency_limit
            .acquire()
            .await
            .map_err(|_| McpError::UpstreamError("Upstream concurrency limit reached".to_string()))?;

        let servers = self.servers.read().await;
        let server = servers.get(server_id).ok_or_else(|| {
            McpError::UpstreamError(format!("Unknown upstream server: {}", server_id))
        })?;

        if server.is_echo {
            return self.forward_to_echo(message).await;
        }

        match &server.transport {
            UpstreamTransport::StreamableHttp => {
                let url = server.url.as_ref().ok_or_else(|| {
                    McpError::UpstreamError(format!("No URL configured for server {}", server_id))
                })?;
                self.forward_to_http(url, message).await
            }
            UpstreamTransport::Sse => {
                let url = server.url.as_ref().ok_or_else(|| {
                    McpError::UpstreamError(format!("No URL configured for server {}", server_id))
                })?;
                // SSE uses the same HTTP POST for client→server messages
                self.forward_to_http(url, message).await
            }
            UpstreamTransport::Stdio => {
                Err(McpError::UpstreamError(
                    "Stdio upstream transport not yet implemented".to_string(),
                ))
            }
        }
    }

    /// Forward a request to an HTTP-based upstream.
    async fn forward_to_http(
        &self,
        url: &str,
        message: JsonRpcMessage,
    ) -> Result<JsonRpcMessage, McpError> {
        let body = message.to_json_string();

        tracing::debug!(upstream_url = %url, "Forwarding request to upstream");

        let response = self
            .http_client
            .post(url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .body(body)
            .send()
            .await
            .map_err(|e| {
                McpError::UpstreamError(format!("Upstream HTTP request failed: {}", e))
            })?;

        let status = response.status();
        let response_body = response.text().await.map_err(|e| {
            McpError::UpstreamError(format!("Failed to read upstream response: {}", e))
        })?;

        if !status.is_success() && status.as_u16() != 202 {
            return Err(McpError::UpstreamError(format!(
                "Upstream returned HTTP {}: {}",
                status, response_body
            )));
        }

        // Parse the response
        let raw: Value = serde_json::from_str(&response_body).map_err(|e| {
            McpError::ParseError(format!("Invalid JSON from upstream: {}", e))
        })?;

        JsonRpcMessage::parse(&raw).map_err(|e| {
            McpError::UpstreamError(format!("Invalid JSON-RPC from upstream: {}", e))
        })
    }

    /// Forward a request to the built-in echo test server.
    async fn forward_to_echo(
        &self,
        message: JsonRpcMessage,
    ) -> Result<JsonRpcMessage, McpError> {
        let method = message.method().unwrap_or("");
        let id = message.id().cloned().unwrap_or(RequestId::Null);

        tracing::debug!(method = %method, "Echo server handling request");

        let result = match method {
            "initialize" => {
                let params = message.params().cloned().unwrap_or(Value::Null);
                let protocol_version = params
                    .get("protocolVersion")
                    .and_then(|v| v.as_str())
                    .unwrap_or("2025-03-26")
                    .to_string();

                json!({
                    "protocolVersion": protocol_version,
                    "capabilities": {
                        "tools": { "listChanged": true },
                        "resources": {},
                        "prompts": {}
                    },
                    "serverInfo": {
                        "name": "mcp-shield-echo",
                        "version": "0.1.0"
                    }
                })
            }
            "tools/list" => {
                json!({
                    "tools": [
                        {
                            "name": "com.echo.echo",
                            "description": "Echoes back the input message",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "message": { "type": "string", "description": "Message to echo" }
                                },
                                "required": ["message"]
                            }
                        },
                        {
                            "name": "com.echo.add",
                            "description": "Adds two numbers together",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "a": { "type": "integer" },
                                    "b": { "type": "integer" }
                                },
                                "required": ["a", "b"]
                            }
                        },
                        {
                            "name": "com.echo.get_time",
                            "description": "Returns the current server time",
                            "inputSchema": {
                                "type": "object",
                                "properties": {}
                            }
                        }
                    ]
                })
            }
            "tools/call" => {
                let params = message.params().cloned().unwrap_or(Value::Null);
                let tool_name = params
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");

                let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);

                match tool_name {
                    "com.echo.echo" => {
                        let msg = arguments
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("no message");
                        json!({
                            "content": [
                                {
                                    "type": "text",
                                    "text": format!("Echo: {}", msg)
                                }
                            ]
                        })
                    }
                    "com.echo.add" => {
                        let a = arguments.get("a").and_then(|v| v.as_i64()).unwrap_or(0);
                        let b = arguments.get("b").and_then(|v| v.as_i64()).unwrap_or(0);
                        json!({
                            "content": [
                                {
                                    "type": "text",
                                    "text": format!("{} + {} = {}", a, b, a + b)
                                }
                            ]
                        })
                    }
                    "com.echo.get_time" => {
                        json!({
                            "content": [
                                {
                                    "type": "text",
                                    "text": chrono::Utc::now().to_rfc3339()
                                }
                            ]
                        })
                    }
                    _ => {
                        return Err(McpError::MethodNotFound(format!(
                            "Echo server: unknown tool '{}'",
                            tool_name
                        )));
                    }
                }
            }
            "ping" => {
                json!({})
            }
            "resources/list" => {
                json!({
                    "resources": []
                })
            }
            "prompts/list" => {
                json!({
                    "prompts": []
                })
            }
            _ => {
                return Err(McpError::MethodNotFound(format!(
                    "Echo server: unknown method '{}'",
                    method
                )));
            }
        };

        Ok(JsonRpcMessage::success_response(id, result))
    }

    /// List all registered upstream servers.
    pub async fn list_servers(&self) -> Vec<String> {
        let servers = self.servers.read().await;
        servers.keys().cloned().collect()
    }

    /// Get the first available server ID.
    pub async fn default_server_id(&self) -> Option<String> {
        let servers = self.servers.read().await;
        servers.keys().next().cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_echo_server_initialize() {
        let proxy = UpstreamProxy::new(10, 10);

        let request = JsonRpcMessage::Request {
            id: RequestId::Integer(1),
            method: "initialize".to_string(),
            params: Some(json!({
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "test", "version": "1.0.0"}
            })),
        };

        let response = proxy.forward_to_echo(request).await.unwrap();
        match response {
            JsonRpcMessage::Success { id, result } => {
                assert_eq!(id, RequestId::Integer(1));
                assert_eq!(result["protocolVersion"], "2025-03-26");
                assert_eq!(result["serverInfo"]["name"], "mcp-shield-echo");
            }
            _ => panic!("Expected Success response"),
        }
    }

    #[tokio::test]
    async fn test_echo_server_tools_list() {
        let proxy = UpstreamProxy::new(10, 10);

        let request = JsonRpcMessage::Request {
            id: RequestId::Integer(2),
            method: "tools/list".to_string(),
            params: None,
        };

        let response = proxy.forward_to_echo(request).await.unwrap();
        match response {
            JsonRpcMessage::Success { result, .. } => {
                let tools = result["tools"].as_array().unwrap();
                assert_eq!(tools.len(), 3);
                assert_eq!(tools[0]["name"], "com.echo.echo");
            }
            _ => panic!("Expected Success response"),
        }
    }

    #[tokio::test]
    async fn test_echo_server_tools_call_echo() {
        let proxy = UpstreamProxy::new(10, 10);

        let request = JsonRpcMessage::Request {
            id: RequestId::Integer(3),
            method: "tools/call".to_string(),
            params: Some(json!({
                "name": "com.echo.echo",
                "arguments": {"message": "hello world"}
            })),
        };

        let response = proxy.forward_to_echo(request).await.unwrap();
        match response {
            JsonRpcMessage::Success { result, .. } => {
                assert!(result["content"][0]["text"].as_str().unwrap().contains("hello world"));
            }
            _ => panic!("Expected Success response"),
        }
    }

    #[tokio::test]
    async fn test_echo_server_ping() {
        let proxy = UpstreamProxy::new(10, 10);

        let request = JsonRpcMessage::Request {
            id: RequestId::Integer(4),
            method: "ping".to_string(),
            params: None,
        };

        let response = proxy.forward_to_echo(request).await.unwrap();
        match response {
            JsonRpcMessage::Success { id, result } => {
                assert_eq!(id, RequestId::Integer(4));
                assert_eq!(result, json!({}));
            }
            _ => panic!("Expected Success response"),
        }
    }
}
