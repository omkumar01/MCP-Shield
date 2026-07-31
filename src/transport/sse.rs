//! Legacy SSE transport for MCP-Shield.
//!
//! Implements the older SSE + HTTP POST transport for backward compatibility
//! with MCP clients that have not yet migrated to Streamable HTTP.
//!
//! - GET `/sse` opens a persistent SSE event stream (server → client)
//! - POST `/messages` sends client → server messages

use crate::auth::scope::ScopeEnforcer;
use crate::gateway::router::McpRouter;
use crate::protocol::jsonrpc::{JsonRpcErrorObj, JsonRpcMessage};
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures::stream::{self};
use serde_json::json;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, mpsc};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;

/// Shared state for the SSE transport.
pub struct SseState {
    /// The MCP router.
    pub router: Arc<McpRouter>,

    /// Map of client_id → response sender.
    pub clients: Arc<RwLock<std::collections::HashMap<String, mpsc::Sender<JsonRpcMessage>>>>,

    /// The scope enforcer.
    pub scope_enforcer: ScopeEnforcer,
}

impl SseState {
    /// Create new SSE transport state.
    pub fn new(router: Arc<McpRouter>, scope_enforcer: ScopeEnforcer) -> Self {
        Self {
            router,
            clients: Arc::new(RwLock::new(std::collections::HashMap::new())),
            scope_enforcer,
        }
    }

    /// Generate a new client ID.
    pub fn generate_client_id() -> String {
        uuid::Uuid::new_v4().to_string()
    }
}

/// Query parameters for the SSE endpoint.
#[derive(Debug, serde::Deserialize)]
pub struct SseQuery {
    /// Optional client ID for reconnection.
    pub client_id: Option<String>,
}

/// GET `/sse` — Opens an SSE event stream.
///
/// The server sends an initial `endpoint` event telling the client where
/// to POST messages, then streams responses and notifications.
pub async fn handle_sse_get(
    State(state): State<Arc<SseState>>,
    Query(query): Query<SseQuery>,
) -> Response {
    let client_id = query
        .client_id
        .unwrap_or_else(SseState::generate_client_id);

    tracing::info!(client_id = %client_id, "New SSE client connected");

    // Create a channel for sending messages to this client
    let (tx, rx) = mpsc::channel::<JsonRpcMessage>(100);

    // Register the client
    {
        let mut clients = state.clients.write().await;
        clients.insert(client_id.clone(), tx);
    }

    // Build the endpoint URL the client should POST to
    let endpoint_url = format!("/messages?client_id={}", client_id);

    // Create the SSE stream
    let initial_event = stream::once(async move {
        Ok::<_, Infallible>(Event::default().event("endpoint").data(endpoint_url))
    });

    let message_stream = ReceiverStream::new(rx).map(|msg| {
        Ok::<_, Infallible>(Event::default().event("message").data(msg.to_json_string()))
    });

    let stream = initial_event.chain(message_stream);

    Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("ping"),
        )
        .into_response()
}

/// POST `/messages` — Receives a client → server message.
///
/// The client POSTs JSON-RPC messages here. The gateway processes them
/// through the router and sends the response back over the SSE stream.
pub async fn handle_sse_post(
    State(state): State<Arc<SseState>>,
    Query(query): Query<SseQuery>,
    body: axum::body::Bytes,
) -> Response {
    let client_id = match query.client_id {
        Some(id) => id,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Missing client_id query parameter"})),
            )
                .into_response();
        }
    };

    // Look up the client's response channel
    let tx = {
        let clients = state.clients.read().await;
        clients.get(&client_id).cloned()
    };

    let tx = match tx {
        Some(tx) => tx,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Unknown or disconnected client"})),
            )
                .into_response();
        }
    };

    // Parse the request body
    let body_str = match std::str::from_utf8(&body) {
        Ok(s) => s,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid UTF-8 in request body"})),
            )
                .into_response();
        }
    };

    let raw: serde_json::Value = match serde_json::from_str(body_str) {
        Ok(v) => v,
        Err(_e) => {
            // Send parse error back over SSE
            let _ = tx.send(JsonRpcMessage::parse_error_response()).await;
            return (StatusCode::ACCEPTED, "").into_response();
        }
    };

    let message = match JsonRpcMessage::parse(&raw) {
        Ok(m) => m,
        Err(e) => {
            let _ = tx
                .send(JsonRpcMessage::Error {
                    id: crate::error::RequestId::Null,
                    error: JsonRpcErrorObj::new(e.code(), e.to_string()),
                })
                .await;
            return (StatusCode::ACCEPTED, "").into_response();
        }
    };

    let is_notification = message.is_notification();

    // Route through the gateway pipeline
    match state
        .router
        .handle_message(message, &state.scope_enforcer, Some(&client_id))
        .await
    {
        Ok(response) => {
            // Send the response back over the SSE stream
            if let Err(e) = tx.send(response).await {
                tracing::warn!(error = %e, client_id = %client_id, "Failed to send SSE response");
            }
        }
        Err(e) => {
            // For non-notifications, send an error response
            if !is_notification && !e.to_string().contains("notification") {
                let _ = tx
                    .send(JsonRpcMessage::Error {
                        id: crate::error::RequestId::Null,
                        error: JsonRpcErrorObj::new(e.code(), e.to_string()),
                    })
                    .await;
            }
        }
    }

    // Always return 202 Accepted for POST messages
    (StatusCode::ACCEPTED, "").into_response()
}

/// Clean up a disconnected client.
pub async fn remove_client(state: &SseState, client_id: &str) {
    let mut clients = state.clients.write().await;
    clients.remove(client_id);
    tracing::info!(client_id = %client_id, "Removed SSE client");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_id_generation() {
        let id1 = SseState::generate_client_id();
        let id2 = SseState::generate_client_id();
        assert_ne!(id1, id2);
    }
}
