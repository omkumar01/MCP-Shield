//! Unified error types for MCP-Shield.
//!
//! All errors are expressed as [`McpError`], which maps to JSON-RPC 2.0 error codes
//! and can be serialized as HTTP responses.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// JSON-RPC 2.0 standard error codes.
pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

/// MCP-Shield custom error codes (in the -32000 to -32099 range).
pub const UNAUTHORIZED: i64 = -32001;
pub const SCOPE_DENIED: i64 = -32002;
pub const SCHEMA_INVALID: i64 = -32003;
pub const REGISTRY_COLLISION: i64 = -32004;
pub const SESSION_LOCKED: i64 = -32005;
pub const EPCA_VIOLATION: i64 = -32006;

/// The top-level error type for MCP-Shield.
///
/// Every failure path in the gateway produces an `McpError`. Each variant
/// carries both a human-readable message and the correct JSON-RPC error code.
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    // ── JSON-RPC protocol errors ────────────────────────────────────
    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Method not found: {0}")]
    MethodNotFound(String),

    #[error("Invalid params: {0}")]
    InvalidParams(String),

    #[error("Internal error: {0}")]
    InternalError(String),

    // ── MCP-Shield custom errors ────────────────────────────────────
    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Scope denied: {0}")]
    ScopeDenied(String),

    #[error("Schema validation failed: {0}")]
    SchemaInvalid(String),

    #[error("Registry collision: {0}")]
    RegistryCollision(String),

    #[error("Session locked: {0}")]
    SessionLocked(String),

    #[error("ePCA constraint violation: {0}")]
    EcpaViolation(String),

    // ── Auth-specific errors ───────────────────────────────────────
    #[error("JWT error: {0}")]
    JwtError(String),

    #[error("OAuth error: {0}")]
    OAuthError(String),

    // ── Transport errors ──────────────────────────────────────────────
    #[error("Transport error: {0}")]
    TransportError(String),

    #[error("Upstream error: {0}")]
    UpstreamError(String),
}

impl McpError {
    /// Returns the JSON-RPC error code for this error variant.
    pub fn code(&self) -> i64 {
        match self {
            Self::ParseError(_) => PARSE_ERROR,
            Self::InvalidRequest(_) => INVALID_REQUEST,
            Self::MethodNotFound(_) => METHOD_NOT_FOUND,
            Self::InvalidParams(_) => INVALID_PARAMS,
            Self::InternalError(_) => INTERNAL_ERROR,
            Self::Unauthorized(_) => UNAUTHORIZED,
            Self::ScopeDenied(_) => SCOPE_DENIED,
            Self::SchemaInvalid(_) => SCHEMA_INVALID,
            Self::RegistryCollision(_) => REGISTRY_COLLISION,
            Self::SessionLocked(_) => SESSION_LOCKED,
            Self::EcpaViolation(_) => EPCA_VIOLATION,
            Self::JwtError(_) => UNAUTHORIZED,
            Self::OAuthError(_) => UNAUTHORIZED,
            Self::TransportError(_) => INTERNAL_ERROR,
            Self::UpstreamError(_) => INTERNAL_ERROR,
        }
    }

    /// Returns the HTTP status code that corresponds to this error.
    pub fn http_status(&self) -> StatusCode {
        match self {
            Self::Unauthorized(_) | Self::JwtError(_) | Self::OAuthError(_) => {
                StatusCode::UNAUTHORIZED
            }
            Self::ScopeDenied(_) => StatusCode::FORBIDDEN,
            Self::InvalidRequest(_) | Self::ParseError(_) => StatusCode::BAD_REQUEST,
            Self::InvalidParams(_) => StatusCode::BAD_REQUEST,
            Self::MethodNotFound(_) => StatusCode::NOT_FOUND,
            Self::SessionLocked(_) => StatusCode::CONFLICT,
            Self::InternalError(_) | Self::UpstreamError(_) | Self::TransportError(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
            Self::SchemaInvalid(_) => StatusCode::BAD_REQUEST,
            Self::RegistryCollision(_) => StatusCode::CONFLICT,
            Self::EcpaViolation(_) => StatusCode::FORBIDDEN,
        }
    }

    /// Convert this error into a JSON-RPC error response object.
    /// If `request_id` is provided, the response includes the correlation ID.
    pub fn to_json_rpc_response(&self, request_id: Option<RequestId>) -> serde_json::Value {
        let mut obj = json!({
            "jsonrpc": "2.0",
            "error": {
                "code": self.code(),
                "message": self.to_string(),
            }
        });
        if let Some(id) = request_id {
            obj["id"] = json!(id);
        } else {
            obj["id"] = json!(null);
        }
        obj
    }
}

/// Request ID type: either a string or integer, per JSON-RPC 2.0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequestId {
    String(String),
    Integer(i64),
    Null,
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RequestId::String(s) => write!(f, "{}", s),
            RequestId::Integer(n) => write!(f, "{}", n),
            RequestId::Null => write!(f, "null"),
        }
    }
}

// We need serde for RequestId
use serde::{Deserialize, Serialize};

impl From<RequestId> for serde_json::Value {
    fn from(id: RequestId) -> Self {
        match id {
            RequestId::String(s) => json!(s),
            RequestId::Integer(n) => json!(n),
            RequestId::Null => json!(null),
        }
    }
}

/// Convert `McpError` into an axum response.
impl IntoResponse for McpError {
    fn into_response(self) -> Response {
        let status = self.http_status();
        let body = Json(json!({
            "jsonrpc": "2.0",
            "error": {
                "code": self.code(),
                "message": self.to_string(),
            },
            "id": null
        }));
        (status, body).into_response()
    }
}

impl From<serde_json::Error> for McpError {
    fn from(err: serde_json::Error) -> Self {
        McpError::ParseError(err.to_string())
    }
}

impl<'a> From<jsonschema::ValidationError<'a>> for McpError {
    fn from(err: jsonschema::ValidationError<'a>) -> Self {
        McpError::SchemaInvalid(err.to_string())
    }
}

impl From<jsonwebtoken::errors::Error> for McpError {
    fn from(err: jsonwebtoken::errors::Error) -> Self {
        McpError::JwtError(err.to_string())
    }
}

/// A structured JSON-RPC error for use in response construction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn with_data(code: i64, message: impl Into<String>, data: serde_json::Value) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(data),
        }
    }
}

/// A complete JSON-RPC error response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcErrorResponse {
    pub jsonrpc: &'static str,
    pub id: RequestId,
    pub error: JsonRpcError,
}

impl JsonRpcErrorResponse {
    pub fn new(id: RequestId, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            error: JsonRpcError::new(code, message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_codes() {
        let err = McpError::ParseError("bad json".into());
        assert_eq!(err.code(), PARSE_ERROR);

        let err = McpError::Unauthorized("no token".into());
        assert_eq!(err.code(), UNAUTHORIZED);

        let err = McpError::ScopeDenied("missing scope".into());
        assert_eq!(err.code(), SCOPE_DENIED);
    }

    #[test]
    fn test_http_status_mapping() {
        let err = McpError::Unauthorized("no token".into());
        assert_eq!(err.http_status(), StatusCode::UNAUTHORIZED);

        let err = McpError::MethodNotFound("foo/bar".into());
        assert_eq!(err.http_status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_json_rpc_error_response() {
        let err = McpError::MethodNotFound("tools/xyz".into());
        let resp = err.to_json_rpc_response(Some(RequestId::Integer(42)));
        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 42);
        assert_eq!(resp["error"]["code"], METHOD_NOT_FOUND);
        assert!(
            resp["error"]["message"]
                .as_str()
                .unwrap()
                .contains("tools/xyz")
        );
    }

    #[test]
    fn test_json_rpc_error_null_id() {
        let err = McpError::ParseError("bad".into());
        let resp = err.to_json_rpc_response(None);
        assert_eq!(resp["id"], json!(null));
    }
}
