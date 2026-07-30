//! Request router for MCP-Shield.
//!
//! Routes incoming MCP method requests to the appropriate handler.
//! Performs method validation, scope checking, schema validation,
//! and upstream forwarding.

use crate::auth::scope::ScopeEnforcer;
use crate::error::{McpError, RequestId, METHOD_NOT_FOUND};
use crate::gateway::proxy::UpstreamProxy;
use crate::gateway::registry::ToolRegistry;
use crate::protocol::jsonrpc::JsonRpcMessage;
use crate::protocol::message::*;
use crate::protocol::schema::SchemaValidator;
use crate::telemetry::metrics::McpMetrics;
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;

/// The request router that orchestrates the gateway's request pipeline.
pub struct McpRouter {
    /// Tool registry for namespace-isolated tool lookups.
    pub registry: Arc<ToolRegistry>,

    /// Upstream proxy for forwarding requests.
    pub proxy: Arc<UpstreamProxy>,

    /// Schema validator for tool argument validation.
    pub schema_validator: Arc<tokio::sync::Mutex<SchemaValidator>>,

    /// Metrics recorder.
    pub metrics: Arc<McpMetrics>,

    /// Server info for initialize responses.
    pub server_name: String,
    pub server_version: String,
}

impl McpRouter {
    /// Create a new MCP router.
    pub fn new(
        registry: Arc<ToolRegistry>,
        proxy: Arc<UpstreamProxy>,
        metrics: Arc<McpMetrics>,
        server_name: String,
        server_version: String,
    ) -> Self {
        Self {
            registry,
            proxy,
            schema_validator: Arc::new(tokio::sync::Mutex::new(SchemaValidator::new())),
            metrics,
            server_name,
            server_version,
        }
    }

    /// Route an incoming JSON-RPC message through the gateway pipeline.
    ///
    /// Pipeline: method validation → scope check → schema validation → upstream proxy
    pub async fn handle_message(
        &self,
        message: JsonRpcMessage,
        scope_enforcer: &ScopeEnforcer,
    ) -> Result<JsonRpcMessage, McpError> {
        let method = message.method().unwrap_or("").to_string();
        let is_request = !message.is_notification();
        let start = Instant::now();

        tracing::debug!(
            method = %method,
            is_notification = !is_request,
            "Routing incoming message"
        );

        // 1. Handle the message based on its type
        let result = match message {
            // ── Notifications (no response expected) ────────────────
            JsonRpcMessage::Notification { method, params } => {
                self.handle_notification(&method, params).await
            }

            // ── Requests (need a response) ──────────────────────────
            JsonRpcMessage::Request { id, method, params } => {
                self.handle_request(id, &method, params, scope_enforcer)
                    .await
            }

            // ── Responses (pass through or error) ───────────────────
            JsonRpcMessage::Success { .. } | JsonRpcMessage::Error { .. } => {
                // Client shouldn't be sending responses to the gateway
                Err(McpError::InvalidRequest(
                    "Gateway received a response message. Only requests and notifications are expected."
                        .to_string(),
                ))
            }

            // ── Batch (flatten and route individually) ───────────────
            JsonRpcMessage::Batch(messages) => {
                // Batch responses are assembled differently; return first error
                // or a combined success. For now, route the first message.
                if let Some(first) = messages.into_iter().next() {
                    return Box::pin(self.handle_message(first, scope_enforcer)).await;
                } else {
                    Err(McpError::InvalidRequest("Empty batch".to_string()))
                }
            }
        };

        // Record metrics
        let status = if result.is_ok() { "success" } else { "error" };
        self.metrics
            .record_request(&method, "gateway", status, start.elapsed());

        result
    }

    /// Handle a notification (no response).
    async fn handle_notification(
        &self,
        method: &str,
        _params: Option<serde_json::Value>,
    ) -> Result<JsonRpcMessage, McpError> {
        match method {
            METHOD_INITIALIZED => {
                tracing::info!("Client completed initialization handshake");
                // No response for notifications
                Err(McpError::InvalidRequest(
                    "notifications/initialized is a notification and should not receive a response"
                        .to_string(),
                ))
            }
            METHOD_EXITED => {
                tracing::info!("Client sent exit notification");
                Err(McpError::InvalidRequest(
                    "notifications/exited is a notification and should not receive a response"
                        .to_string(),
                ))
            }
            METHOD_TOOLS_LIST_CHANGED => {
                tracing::info!("Tools list changed notification received");
                Err(McpError::InvalidRequest(
                    "Notification should not receive a response".to_string(),
                ))
            }
            METHOD_RESOURCES_LIST_CHANGED => {
                tracing::info!("Resources list changed notification received");
                Err(McpError::InvalidRequest(
                    "Notification should not receive a response".to_string(),
                ))
            }
            METHOD_PROMPTS_LIST_CHANGED => {
                tracing::info!("Prompts list changed notification received");
                Err(McpError::InvalidRequest(
                    "Notification should not receive a response".to_string(),
                ))
            }
            _ => {
                tracing::warn!(method = %method, "Unknown notification method");
                Err(McpError::InvalidRequest(
                    "Notification should not receive a response".to_string(),
                ))
            }
        }
    }

    /// Handle a request (must produce a response).
    async fn handle_request(
        &self,
        id: RequestId,
        method: &str,
        params: Option<serde_json::Value>,
        scope_enforcer: &ScopeEnforcer,
    ) -> Result<JsonRpcMessage, McpError> {
        // Check if the method is known to MCP
        if !is_valid_method(method) && !method.starts_with("notifications/") {
            return Ok(JsonRpcMessage::method_not_found_response(id, method));
        }

        // Scope enforcement
        scope_enforcer.check_method(method)?;

        match method {
            METHOD_INITIALIZE => self.handle_initialize(id, params).await,
            METHOD_PING => Ok(JsonRpcMessage::success_response(id, json!({}))),
            METHOD_TOOLS_LIST => self.handle_tools_list(id).await,
            METHOD_TOOLS_CALL => self.handle_tools_call(id, params, scope_enforcer).await,
            METHOD_RESOURCES_LIST => {
                // Proxy to upstream
                self.proxy_to_upstream(id, method, params).await
            }
            METHOD_PROMPTS_LIST => {
                // Proxy to upstream
                self.proxy_to_upstream(id, method, params).await
            }
            METHOD_SHUTDOWN => {
                // Shutdown requires admin scope (already checked above)
                tracing::info!("Shutdown requested");
                Ok(JsonRpcMessage::success_response(id, json!({})))
            }
            // Notifications sent as requests (malformed) → still handle gracefully
            METHOD_INITIALIZED => Err(McpError::InvalidRequest(
                "notifications/initialized should be a notification (no id)".to_string(),
            )),
            _ => self.proxy_to_upstream(id, method, params).await,
        }
    }

    /// Handle the initialize request.
    async fn handle_initialize(
        &self,
        id: RequestId,
        params: Option<serde_json::Value>,
    ) -> Result<JsonRpcMessage, McpError> {
        let _params: InitializeParams = serde_json::from_value(
            params.unwrap_or(json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "unknown", "version": "0.0.0"}
            })),
        )
        .map_err(|e| McpError::InvalidParams(format!("Invalid initialize params: {}", e)))?;

        tracing::info!(
            "Client initialized with MCP protocol version {}",
            _params.protocol_version
        );

        let result = json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {
                "tools": { "listChanged": true },
                "resources": {},
                "prompts": {}
            },
            "serverInfo": {
                "name": self.server_name,
                "version": self.server_version
            },
            "instructions": "MCP-Shield security gateway. All requests are validated, authenticated, and authorized."
        });

        Ok(JsonRpcMessage::success_response(id, result))
    }

    /// Handle tools/list — aggregate tools from the registry.
    async fn handle_tools_list(
        &self,
        id: RequestId,
    ) -> Result<JsonRpcMessage, McpError> {
        let tools = self.registry.list_tools().await;

        let result = json!({
            "tools": tools
        });

        Ok(JsonRpcMessage::success_response(id, result))
    }

    /// Handle tools/call — validate schema and forward to upstream.
    async fn handle_tools_call(
        &self,
        id: RequestId,
        params: Option<serde_json::Value>,
        scope_enforcer: &ScopeEnforcer,
    ) -> Result<JsonRpcMessage, McpError> {
        let params = params.unwrap_or(json!({}));

        // Parse tool name
        let tool_name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                McpError::InvalidParams("tools/call requires a 'name' parameter".to_string())
            })?
            .to_string();

        // Check tool-level scope access
        scope_enforcer.check_tool_access(&tool_name)?;

        // Look up the tool in the registry
        let registered = self
            .registry
            .lookup_any(&tool_name)
            .await?
            .ok_or_else(|| {
                McpError::MethodNotFound(format!("Tool '{}' not found in registry", tool_name))
            })?;

        // Validate arguments against the tool's inputSchema
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
        if arguments != json!(null) {
            let mut validator = self.schema_validator.lock().await;
            if let Err(e) = validator.validate(&registered.tool.input_schema, &arguments).await {
                tracing::warn!(
                    tool = %tool_name,
                    error = %e,
                    "Schema validation failed for tool call"
                );
                self.metrics.increment_validation_failure("schema_mismatch");
                return Err(e);
            }
        }

        // Forward to the upstream server that owns this tool
        let server_id = &registered.server_id;
        let request_params = Some(params.clone());
        let request = JsonRpcMessage::Request {
            id: id.clone(),
            method: METHOD_TOOLS_CALL.to_string(),
            params: request_params,
        };

        match self.proxy.forward_request(server_id, request).await {
            Ok(response) => Ok(response),
            Err(e) => {
                tracing::error!(
                    tool = %tool_name,
                    server = %server_id,
                    error = %e,
                    "Upstream request failed"
                );
                Err(e)
            }
        }
    }

    /// Proxy a generic request to the default upstream server.
    async fn proxy_to_upstream(
        &self,
        id: RequestId,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<JsonRpcMessage, McpError> {
        let server_id = self.proxy.default_server_id().await.ok_or_else(|| {
            McpError::UpstreamError("No upstream servers configured".to_string())
        })?;

        let request = JsonRpcMessage::Request {
            id,
            method: method.to_string(),
            params,
        };

        self.proxy.forward_request(&server_id, request).await
    }
}

// Helper to allow async in match arms
use std::pin::Pin;
use futures::future::Future;

fn boxed<F: Future<Output = Result<JsonRpcMessage, McpError>> + Send + 'static>(
    f: F,
) -> Pin<Box<F>> {
    Box::pin(f)
}
