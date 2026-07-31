//! Request router for MCP-Shield.
//!
//! Routes incoming MCP method requests to the appropriate handler.
//! Performs method validation, scope checking, schema validation,
//! Cedar policy evaluation, session locking, ePCA guardrails, egress sanitization,
//! and upstream forwarding.

use crate::auth::scope::ScopeEnforcer;
use crate::error::{McpError, RequestId};
use crate::gateway::proxy::UpstreamProxy;
use crate::gateway::registry::ToolRegistry;
use crate::guardrail::{EcpaGuardrail, EgressInspector};
use crate::policy::CedarAuthorizer;
use crate::protocol::jsonrpc::JsonRpcMessage;
use crate::protocol::message::*;
use crate::protocol::schema::SchemaValidator;
use crate::session::state::{ContextScope, SessionManager, Visibility};
use crate::telemetry::metrics::McpMetrics;
use crate::telemetry::producer::{AuditEvent, AuthDecision, EventProducer};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;

/// The request router that orchestrates the gateway's request pipeline.
pub struct McpRouter {
    /// Tool registry for namespace-isolated tool lookups.
    pub registry: Arc<ToolRegistry>,

    /// Upstream proxy for forwarding requests.
    pub proxy: Arc<UpstreamProxy>,

    /// Schema validator for tool argument validation.
    pub schema_validator: Arc<Mutex<SchemaValidator>>,

    /// Metrics recorder.
    pub metrics: Arc<McpMetrics>,

    /// Server info for initialize responses.
    pub server_name: String,
    pub server_version: String,

    /// Optional Cedar policy authorizer (Phase 2).
    pub authorizer: Option<Arc<dyn CedarAuthorizer>>,

    /// Optional session manager for context locking (Phase 2).
    pub sessions: Option<Arc<dyn SessionManager>>,

    /// Optional audit event producer (Phase 2).
    pub audit_producer: Option<Arc<dyn EventProducer>>,

    /// Optional ePCA guardrail for pre-execution constraint checking (Phase 3).
    pub ecpa_guardrail: Option<Arc<dyn EcpaGuardrail>>,

    /// Optional egress inspector for post-execution response sanitization (Phase 3).
    pub egress_inspector: Option<Arc<dyn EgressInspector>>,
}

impl McpRouter {
    /// Create a new MCP router (backwards compatible, Phase 1 only).
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
            schema_validator: Arc::new(Mutex::new(SchemaValidator::new())),
            metrics,
            server_name,
            server_version,
            authorizer: None,
            sessions: None,
            audit_producer: None,
            ecpa_guardrail: None,
            egress_inspector: None,
        }
    }

    /// Create a new MCP router with Cedar authorizer and session manager (Phase 2+).
    pub fn with_policy(
        registry: Arc<ToolRegistry>,
        proxy: Arc<UpstreamProxy>,
        metrics: Arc<McpMetrics>,
        server_name: String,
        server_version: String,
        authorizer: Option<Arc<dyn CedarAuthorizer>>,
        sessions: Option<Arc<dyn SessionManager>>,
    ) -> Self {
        Self {
            registry,
            proxy,
            schema_validator: Arc::new(Mutex::new(SchemaValidator::new())),
            metrics,
            server_name,
            server_version,
            authorizer,
            sessions,
            audit_producer: None,
            ecpa_guardrail: None,
            egress_inspector: None,
        }
    }

    /// Create a new MCP router with full Phase 2+ features.
    pub fn with_full_config(
        registry: Arc<ToolRegistry>,
        proxy: Arc<UpstreamProxy>,
        metrics: Arc<McpMetrics>,
        server_name: String,
        server_version: String,
        authorizer: Option<Arc<dyn CedarAuthorizer>>,
        sessions: Option<Arc<dyn SessionManager>>,
        audit_producer: Option<Arc<dyn EventProducer>>,
    ) -> Self {
        Self {
            registry,
            proxy,
            schema_validator: Arc::new(Mutex::new(SchemaValidator::new())),
            metrics,
            server_name,
            server_version,
            authorizer,
            sessions,
            audit_producer,
            ecpa_guardrail: None,
            egress_inspector: None,
        }
    }

    /// Create a new MCP router with full Phase 3+ features including guardrails.
    pub fn with_guardrails(
        registry: Arc<ToolRegistry>,
        proxy: Arc<UpstreamProxy>,
        metrics: Arc<McpMetrics>,
        server_name: String,
        server_version: String,
        authorizer: Option<Arc<dyn CedarAuthorizer>>,
        sessions: Option<Arc<dyn SessionManager>>,
        audit_producer: Option<Arc<dyn EventProducer>>,
        ecpa_guardrail: Option<Arc<dyn EcpaGuardrail>>,
        egress_inspector: Option<Arc<dyn EgressInspector>>,
    ) -> Self {
        Self {
            registry,
            proxy,
            schema_validator: Arc::new(Mutex::new(SchemaValidator::new())),
            metrics,
            server_name,
            server_version,
            authorizer,
            sessions,
            audit_producer,
            ecpa_guardrail,
            egress_inspector,
        }
    }

    /// Route an incoming JSON-RPC message through the gateway pipeline.
    ///
    /// Pipeline: method validation → scope check → Cedar policy → session lock → schema validation → ePCA → upstream proxy
    pub async fn handle_message(
        &self,
        message: JsonRpcMessage,
        scope_enforcer: &ScopeEnforcer,
        session_id: Option<&str>,
    ) -> Result<JsonRpcMessage, McpError> {
        let method = message.method().unwrap_or("").to_string();
        let is_request = !message.is_notification();
        let start = Instant::now();

        // Extract fields we need for audit logging before the match consumes the message
        let _request_id = message.id().map(|id| id.to_string());
        let message_value = message.to_value(); // Clone for audit
        let message_for_audit = message.clone(); // Keep a clone for the match

        tracing::debug!(
            method = %method,
            is_notification = !is_request,
            "Routing incoming message"
        );

        // 1. Handle the message based on its type
        let result = match message_for_audit {
            // ── Notifications (no response expected) ────────────────
            JsonRpcMessage::Notification { method, params } => {
                self.handle_notification(&method, params).await
            }

            // ── Requests (need a response) ──────────────────────────
            JsonRpcMessage::Request { id, method, params } => {
                self.handle_request(id, &method, params, scope_enforcer, session_id)
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
                    return Box::pin(self.handle_message(first, scope_enforcer, session_id)).await;
                } else {
                    Err(McpError::InvalidRequest("Empty batch".to_string()))
                }
            }
        };

        // Record metrics
        let status = if result.is_ok() { "success" } else { "error" };
        self.metrics
            .record_request(&method, "gateway", status, start.elapsed());

        // Publish audit event (Phase 2)
        if let Some(ref producer) = self.audit_producer {
            let decision = match (&result, status) {
                (Ok(_), "success") => AuthDecision::Allow,
                (Err(e), _) if e.code() == -32005 => AuthDecision::Block, // Session locked
                (Err(e), _) if e.code() == -32006 => AuthDecision::Block, // ePCA violation
                (Err(e), _) if e.code() == -32002 => AuthDecision::DenyPolicy, // Cedar deny
                (Err(e), _) if e.code() == -32002 || e.to_string().contains("Scope denied") => {
                    AuthDecision::Deny
                }
                _ => AuthDecision::Deny,
            };

            let request_id = message.id().map(|id| id.to_string());
            let principal = scope_enforcer
                .granted_scopes()
                .iter()
                .next()
                .cloned()
                .or_else(|| session_id.map(|s| s.to_string()));

            let scopes: Vec<String> = scope_enforcer.granted_scopes().iter().cloned().collect();

            let event = AuditEvent {
                event_id: uuid::Uuid::new_v4().to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                method: method.clone(),
                request_id,
                session_id: session_id.map(|s| s.to_string()),
                principal,
                scopes,
                decision,
                request_payload: Some(message_value.clone()),
                response_payload: result.as_ref().ok().map(|r| r.to_value()),
                error_code: result.as_ref().err().map(|e| e.code()),
                duration_ms: start.elapsed().as_millis() as u64,
                transport: if session_id.map(|s| s.starts_with("stdio")).unwrap_or(false) {
                    "stdio".to_string()
                } else {
                    "http".to_string()
                },
            };

            // Fire-and-forget - don't block on audit
            let _ = producer.publish_audit_event(event).await;
        }

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
        session_id: Option<&str>,
    ) -> Result<JsonRpcMessage, McpError> {
        // Check if the method is known to MCP
        if !is_valid_method(method) && !method.starts_with("notifications/") {
            return Ok(JsonRpcMessage::method_not_found_response(id, method));
        }

        // Scope enforcement
        scope_enforcer.check_method(method)?;

        // Cedar policy evaluation (Phase 2)
        if let Some(ref authorizer) = self.authorizer {
            // Extract scopes from the scope enforcer
            let scopes: Vec<String> = scope_enforcer.granted_scopes().iter().cloned().collect();

            let auth_req = crate::policy::AuthorizationRequest {
                principal: scope_enforcer
                    .granted_scopes()
                    .iter()
                    .next()
                    .cloned()
                    .unwrap_or_else(|| "anonymous".to_string()),
                action: method.to_string(),
                resource: params
                    .as_ref()
                    .and_then(|p| p.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("any")
                    .to_string(),
                context: {
                    let mut ctx = std::collections::HashMap::new();
                    if !scopes.is_empty() {
                        ctx.insert("scopes".to_string(), serde_json::json!(scopes));
                    }
                    ctx
                },
            };

            match authorizer.evaluate(&auth_req).await {
                Ok(resp) => {
                    if resp.decision == crate::policy::Decision::Deny {
                        self.metrics.increment_blocked_request("cedar_deny");
                        return Err(McpError::ScopeDenied(format!(
                            "Cedar policy denied: {}",
                            resp.diagnostics.join(", ")
                        )));
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "Cedar evaluation error");
                    self.metrics.increment_blocked_request("cedar_error");
                    return Err(McpError::InternalError(format!(
                        "Policy evaluation failed: {}",
                        e
                    )));
                }
            }
        }

        match method {
            METHOD_INITIALIZE => self.handle_initialize(id, params).await,
            METHOD_PING => Ok(JsonRpcMessage::success_response(id, json!({}))),
            METHOD_TOOLS_LIST => self.handle_tools_list(id).await,
            METHOD_TOOLS_CALL => {
                self.handle_tools_call(id, params, scope_enforcer, session_id)
                    .await
            }
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
        let _params: InitializeParams = serde_json::from_value(params.unwrap_or(json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {"name": "unknown", "version": "0.0.0"}
        })))
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
    async fn handle_tools_list(&self, id: RequestId) -> Result<JsonRpcMessage, McpError> {
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
        session_id: Option<&str>,
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
        let registered = self.registry.lookup_any(&tool_name).await?.ok_or_else(|| {
            McpError::MethodNotFound(format!("Tool '{}' not found in registry", tool_name))
        })?;

        // Validate arguments against the tool's inputSchema
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
        if arguments != json!(null) {
            let mut validator = self.schema_validator.lock().await;
            if let Err(e) = validator
                .validate(&registered.tool.input_schema, &arguments)
                .await
            {
                tracing::warn!(
                    tool = %tool_name,
                    error = %e,
                    "Schema validation failed for tool call"
                );
                self.metrics.increment_validation_failure("schema_mismatch");
                return Err(e);
            }
        }

        // ePCA guardrail (Phase 3) - pre-execution constraint checking
        if let Some(ref guardrail) = self.ecpa_guardrail {
            match guardrail.evaluate_constraints(&tool_name, &arguments).await {
                Ok(result) => {
                    if !result.satisfied {
                        self.metrics.increment_blocked_request("ecpa_violation");
                        return Err(McpError::EcpaViolation(format!(
                            "ePCA constraint violated: {}",
                            result
                                .evaluations
                                .iter()
                                .filter(|e| !e.satisfied)
                                .map(|e| e.explanation.clone())
                                .collect::<Vec<_>>()
                                .join("; ")
                        )));
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "ePCA evaluation error");
                    self.metrics.increment_blocked_request("ecpa_error");
                    return Err(McpError::EcpaViolation(format!(
                        "ePCA evaluation failed: {}",
                        e
                    )));
                }
            }
        }

        // Session context locking (Phase 2) - check if enabled in config
        if let (Some(ref sessions), Some(sid)) = (self.sessions.as_ref(), session_id) {
            // Derive a context scope from the tool name (prefix = context)
            let tool_prefix = tool_name.split(':').next().unwrap_or("unknown");
            let context_scope = ContextScope {
                context_type: "mcp_tool".to_string(),
                identifier: tool_prefix.to_string(),
                visibility: Visibility::Private, // Default to private for safety
            };

            match sessions.check_context_access(sid, &context_scope).await {
                Ok(crate::session::state::AccessCheckResult::Allow) => {}
                Ok(crate::session::state::AccessCheckResult::Deny { reason, locked_to }) => {
                    self.metrics.increment_blocked_request("session_lock");
                    return Err(McpError::SessionLocked(format!(
                        "Session locked to {} context '{}'. {}",
                        locked_to.visibility.as_str(),
                        locked_to.identifier,
                        reason
                    )));
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Session check error");
                }
            }

            // Log the tool call for monitoring
            let record = crate::session::state::ToolCallRecord {
                tool_name: tool_name.clone(),
                arguments: arguments.clone(),
                timestamp: chrono::Utc::now(),
            };
            let _ = sessions.log_tool_call(sid, record).await;
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
            Ok(response) => {
                // Egress inspection (Phase 3) - post-execution response sanitization
                if let Some(ref inspector) = self.egress_inspector {
                    // Extract content from response for inspection
                    let content = if let JsonRpcMessage::Success { result, .. } = &response {
                        // Extract content blocks from the result
                        if let Some(content_arr) = result.get("content").and_then(|v| v.as_array())
                        {
                            content_arr.clone()
                        } else {
                            vec![result.clone()]
                        }
                    } else {
                        vec![]
                    };

                    if !content.is_empty() {
                        let inspectable = crate::guardrail::InspectableResult {
                            tool_name: tool_name.clone(),
                            content,
                            server_id: server_id.clone(),
                        };

                        match inspector.sanitize_response(&inspectable).await {
                            Ok(inspection_result) => {
                                if inspection_result.modified {
                                    // Rebuild response with sanitized content
                                    if let JsonRpcMessage::Success { id, result } = response {
                                        let mut new_result = result.clone();
                                        if let Some(content_field) = new_result.get_mut("content") {
                                            *content_field =
                                                Value::Array(inspection_result.sanitized_content);
                                        }
                                        return Ok(JsonRpcMessage::Success {
                                            id,
                                            result: new_result,
                                        });
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "Egress inspection error");
                            }
                        }
                    }
                }
                Ok(response)
            }
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
        let server_id =
            self.proxy.default_server_id().await.ok_or_else(|| {
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
use futures::future::Future;
use std::pin::Pin;

fn boxed<F: Future<Output = Result<JsonRpcMessage, McpError>> + Send + 'static>(
    f: F,
) -> Pin<Box<F>> {
    Box::pin(f)
}
