//! Streamable HTTP transport for MCP-Shield (MCP spec 2025-03-26).
//!
//! Implements the current MCP HTTP transport: JSON-RPC messages are sent
//! as HTTP POST requests to an `/mcp` endpoint. Session management uses
//! the `Mcp-Session-Id` HTTP header.

use crate::auth::scope::ScopeEnforcer;
use crate::error::{McpError, RequestId};
use crate::gateway::router::McpRouter;
use crate::protocol::jsonrpc::{JsonRpcErrorObj, JsonRpcMessage};
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// HTTP header name for the MCP session ID.
pub const MCP_SESSION_ID_HEADER: &str = "mcp-session-id";

/// Shared state for the Streamable HTTP transport.
pub struct StreamableHttpState {
    /// The MCP router that processes messages.
    pub router: Arc<McpRouter>,

    /// Active sessions keyed by session ID.
    pub sessions: Arc<RwLock<HashMap<String, SessionState>>>,

    /// The scope enforcer for the current request (per-session in production).
    pub scope_enforcer: ScopeEnforcer,
}

/// Per-session state.
#[derive(Debug, Clone)]
pub struct SessionState {
    /// When the session was created.
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// Last activity timestamp.
    pub last_activity: chrono::DateTime<chrono::Utc>,

    /// Whether the session has completed the initialize handshake.
    pub initialized: bool,
}

impl StreamableHttpState {
    /// Create new transport state.
    pub fn new(router: Arc<McpRouter>, scope_enforcer: ScopeEnforcer) -> Self {
        Self {
            router,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            scope_enforcer,
        }
    }

    /// Generate a new session ID.
    pub fn generate_session_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }
}

/// POST handler for the `/mcp` endpoint.
///
/// Processes a JSON-RPC message from the request body, manages sessions
/// via the `Mcp-Session-Id` header, and returns the appropriate response.
pub async fn handle_mcp_post(
    State(state): State<Arc<StreamableHttpState>>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    // Parse the body as JSON
    let body_str = match std::str::from_utf8(&body) {
        Ok(s) => s,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "Invalid UTF-8 in request body"),
    };

    let raw: serde_json::Value = match serde_json::from_str(body_str) {
        Ok(v) => v,
        Err(e) => {
            return parse_error_response(&format!("Invalid JSON: {}", e));
        }
    };

    // Parse the JSON-RPC message
    let message = match JsonRpcMessage::parse(&raw) {
        Ok(msg) => msg,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to parse JSON-RPC message");
            return jsonrpc_error_response(e.code(), &e.to_string());
        }
    };

    // Extract or generate session ID
    let session_id = headers
        .get(MCP_SESSION_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Validate session if ID is provided
    if let Some(ref sid) = session_id {
        let sessions = state.sessions.read().await;
        if !sessions.contains_key(sid) {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "jsonrpc": "2.0",
                    "error": {"code": -32000, "message": "Unknown or expired session"}
                })),
            )
                .into_response();
        }
    }

    let is_initialize = message.method() == Some("initialize");

    // Route the message through the gateway pipeline
    let routing_result = state
        .router
        .handle_message(message, &state.scope_enforcer)
        .await;

    // Handle notifications (no response body, return 202)
    if routing_result.as_ref().is_err()
        && routing_result
            .as_ref()
            .unwrap_err()
            .to_string()
            .contains("notification")
    {
        // For initialize, create a session even though initialized is a notification
        let response = (StatusCode::ACCEPTED, "").into_response();
        return response;
    }

    let response_message = match routing_result {
        Ok(ref msg) => msg,
        Err(e) => {
            tracing::warn!(error = %e, "Message routing failed");
            return jsonrpc_error_response(e.code(), &e.to_string());
        }
    };

    // For initialize requests, create a new session and return the session ID header
    let mut response = (StatusCode::OK, Json(response_message.to_value())).into_response();

    if is_initialize {
        let new_session_id = StreamableHttpState::generate_session_id();
        let mut sessions = state.sessions.write().await;
        sessions.insert(
            new_session_id.clone(),
            SessionState {
                created_at: chrono::Utc::now(),
                last_activity: chrono::Utc::now(),
                initialized: false,
            },
        );
        response.headers_mut().insert(
            HeaderName::from_static(MCP_SESSION_ID_HEADER),
            HeaderValue::from_str(&new_session_id).unwrap(),
        );
        tracing::info!(session_id = %new_session_id, "Created new MCP session");
    } else if let Some(ref sid) = session_id {
        // Update last activity timestamp
        let mut sessions = state.sessions.write().await;
        if let Some(session) = sessions.get_mut(sid) {
            session.last_activity = chrono::Utc::now();
            // Mark as initialized after the initialized notification
            if routing_result.is_ok() {
                session.initialized = true;
            }
        }
    }

    response
}

/// Build a parse error response (JSON-RPC code -32700).
fn parse_error_response(message: &str) -> Response {
    (
        StatusCode::OK,
        Json(json!({
            "jsonrpc": "2.0",
            "id": null,
            "error": {"code": -32700, "message": message}
        })),
    )
        .into_response()
}

/// Build a generic JSON-RPC error response.
fn jsonrpc_error_response(code: i64, message: &str) -> Response {
    (
        StatusCode::OK,
        Json(json!({
            "jsonrpc": "2.0",
            "id": null,
            "error": {"code": code, "message": message}
        })),
    )
        .into_response()
}

/// Build a simple error response with a status code and message.
fn error_response(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error": message}))).into_response()
}

/// DELETE handler for the `/mcp` endpoint — terminates a session.
pub async fn handle_mcp_delete(
    State(state): State<Arc<StreamableHttpState>>,
    headers: HeaderMap,
) -> Response {
    let session_id = headers
        .get(MCP_SESSION_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    if let Some(sid) = session_id {
        let mut sessions = state.sessions.write().await;
        if sessions.remove(&sid).is_some() {
            tracing::info!(session_id = %sid, "Terminated MCP session");
            return (StatusCode::OK, Json(json!({"status": "terminated"}))).into_response();
        }
    }

    (
        StatusCode::NOT_FOUND,
        Json(json!({"error": "Session not found"})),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_id_generation() {
        let id1 = StreamableHttpState::generate_session_id();
        let id2 = StreamableHttpState::generate_session_id();
        assert_ne!(id1, id2);
        assert!(id1.len() > 0);
    }
}
