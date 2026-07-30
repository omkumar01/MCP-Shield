//! JSON-RPC 2.0 message parser and serializer.
//!
//! Implements the JSON-RPC 2.0 specification as used by the Model Context Protocol.
//! Handles requests, responses, notifications, errors, and batch messages.

use crate::error::{McpError, RequestId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// The JSON-RPC protocol version. Always "2.0".
pub const JSONRPC_VERSION: &str = "2.0";

/// JSON-RPC 2.0 standard error codes.
pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;

/// A parsed JSON-RPC 2.0 message.
#[derive(Debug, Clone)]
pub enum JsonRpcMessage {
    /// A request expects a response. Always has an `id`.
    Request {
        id: RequestId,
        method: String,
        params: Option<Value>,
    },

    /// A notification does not expect a response. Has no `id`.
    Notification {
        method: String,
        params: Option<Value>,
    },

    /// A successful response mirrors the request's `id`.
    Success {
        id: RequestId,
        result: Value,
    },

    /// An error response mirrors the request's `id`.
    Error {
        id: RequestId,
        error: JsonRpcErrorObj,
    },

    /// A batch of messages (JSON array).
    Batch(Vec<JsonRpcMessage>),
}

/// JSON-RPC error object structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcErrorObj {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcErrorObj {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn with_data(code: i64, message: impl Into<String>, data: Value) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(data),
        }
    }
}

impl fmt::Display for JsonRpcMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request { id, method, .. } => write!(f, "Request(id={:?}, method={})", id, method),
            Self::Notification { method, .. } => write!(f, "Notification(method={})", method),
            Self::Success { id, .. } => write!(f, "Success(id={:?})", id),
            Self::Error { id, error, .. } => {
                write!(f, "Error(id={:?}, code={}, msg={})", id, error.code, error.message)
            }
            Self::Batch(msgs) => write!(f, "Batch({} messages)", msgs.len()),
        }
    }
}

/// Intermediate raw representation for deserialization.
#[derive(Deserialize)]
struct RawMessage {
    #[serde(default)]
    jsonrpc: Option<String>,
    id: Option<RequestId>,
    method: Option<String>,
    #[serde(default)]
    params: Option<Value>,
    result: Option<Value>,
    error: Option<JsonRpcErrorObj>,
}

impl JsonRpcMessage {
    /// Parse a JSON-RPC 2.0 message from a raw JSON value.
    ///
    /// Returns `Ok(JsonRpcMessage)` on success, or `Err(McpError)` with the
    /// appropriate JSON-RPC error code on failure.
    pub fn parse(raw: &Value) -> Result<JsonRpcMessage, McpError> {
        // Batch: JSON array
        if let Some(arr) = raw.as_array() {
            if arr.is_empty() {
                return Err(McpError::InvalidRequest(
                    "Invalid Request: empty batch".to_string(),
                ));
            }
            let mut messages = Vec::with_capacity(arr.len());
            for item in arr {
                messages.push(Self::parse_single(item)?);
            }
            return Ok(JsonRpcMessage::Batch(messages));
        }

        // Single message
        Self::parse_single(raw)
    }

    /// Parse a single JSON-RPC message (not a batch).
    fn parse_single(raw: &Value) -> Result<JsonRpcMessage, McpError> {
        // Must be a JSON object
        if !raw.is_object() {
            return Err(McpError::InvalidRequest(
                "Invalid Request: message must be a JSON object".to_string(),
            ));
        }

        // Check jsonrpc version field
        let version = raw
            .get("jsonrpc")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if version != JSONRPC_VERSION {
            return Err(McpError::InvalidRequest(format!(
                "Invalid Request: expected jsonrpc \"2.0\", got \"{}\"",
                version
            )));
        }

        let has_id = raw.get("id").is_some();
        let has_method = raw.get("method").is_some();
        let has_result = raw.get("result").is_some();
        let has_error = raw.get("error").is_some();

        // Classify the message type
        if has_method && !has_result && !has_error {
            // It's a request or notification
            let method = raw
                .get("method")
                .unwrap()
                .as_str()
                .ok_or_else(|| {
                    McpError::InvalidRequest("Invalid Request: method must be a string".to_string())
                })?
                .to_string();

            let params = raw.get("params").cloned();

            if has_id {
                let id: RequestId = serde_json::from_value(raw.get("id").unwrap().clone())
                    .map_err(|e| {
                        McpError::InvalidRequest(format!(
                            "Invalid Request: id must be string, integer, or null: {}",
                            e
                        ))
                    })?;

                // id must not be null for requests
                if matches!(id, RequestId::Null) {
                    return Err(McpError::InvalidRequest(
                        "Invalid Request: id must not be null".to_string(),
                    ));
                }

                Ok(JsonRpcMessage::Request {
                    id,
                    method,
                    params,
                })
            } else {
                Ok(JsonRpcMessage::Notification { method, params })
            }
        } else if (has_result || has_error) && has_id {
            // It's a response
            let id: RequestId = serde_json::from_value(raw.get("id").unwrap().clone())
                .map_err(|e| {
                    McpError::InvalidRequest(format!(
                        "Invalid Request: id must be string, integer, or null: {}",
                        e
                    ))
                })?;

            if let Some(error) = raw.get("error") {
                let error_obj: JsonRpcErrorObj = serde_json::from_value(error.clone())
                    .map_err(|e| {
                        McpError::InvalidRequest(format!(
                            "Invalid Request: malformed error object: {}",
                            e
                        ))
                    })?;
                Ok(JsonRpcMessage::Error {
                    id,
                    error: error_obj,
                })
            } else {
                let result = raw.get("result").cloned().unwrap_or(Value::Null);
                Ok(JsonRpcMessage::Success { id, result })
            }
        } else {
            Err(McpError::InvalidRequest(
                "Invalid Request: message must be a request, notification, or response"
                    .to_string(),
            ))
        }
    }

    /// Parse a JSON string into a JSON-RPC message.
    pub fn from_str(s: &str) -> Result<JsonRpcMessage, McpError> {
        let raw: Value =
            serde_json::from_str(s).map_err(|e| McpError::ParseError(e.to_string()))?;
        Self::parse(&raw)
    }

    /// Serialize this message to a JSON string.
    pub fn to_json_string(&self) -> String {
        serde_json::to_string(&self.to_value()).unwrap_or_else(|_| {
            serde_json::json!({
                "jsonrpc": "2.0",
                "error": {"code": PARSE_ERROR, "message": "Serialization error"}
            })
            .to_string()
        })
    }

    /// Convert this message to a JSON value.
    pub fn to_value(&self) -> Value {
        match self {
            JsonRpcMessage::Request {
                id,
                method,
                params,
            } => {
                let mut obj = json!({
                    "jsonrpc": JSONRPC_VERSION,
                    "id": id,
                    "method": method,
                });
                if let Some(p) = params {
                    obj["params"] = p.clone();
                }
                obj
            }
            JsonRpcMessage::Notification { method, params } => {
                let mut obj = json!({
                    "jsonrpc": JSONRPC_VERSION,
                    "method": method,
                });
                if let Some(p) = params {
                    obj["params"] = p.clone();
                }
                obj
            }
            JsonRpcMessage::Success { id, result } => {
                json!({
                    "jsonrpc": JSONRPC_VERSION,
                    "id": id,
                    "result": result,
                })
            }
            JsonRpcMessage::Error { id, error } => {
                json!({
                    "jsonrpc": JSONRPC_VERSION,
                    "id": id,
                    "error": error,
                })
            }
            JsonRpcMessage::Batch(messages) => {
                Value::Array(messages.iter().map(|m| m.to_value()).collect())
            }
        }
    }

    /// Get the request ID if this is a request, success response, or error response.
    pub fn id(&self) -> Option<&RequestId> {
        match self {
            JsonRpcMessage::Request { id, .. } => Some(id),
            JsonRpcMessage::Success { id, .. } => Some(id),
            JsonRpcMessage::Error { id, .. } => Some(id),
            JsonRpcMessage::Notification { .. } => None,
            JsonRpcMessage::Batch(_) => None,
        }
    }

    /// Get the method name if this is a request or notification.
    pub fn method(&self) -> Option<&str> {
        match self {
            JsonRpcMessage::Request { method, .. } => Some(method),
            JsonRpcMessage::Notification { method, .. } => Some(method),
            _ => None,
        }
    }

    /// Get the params if this is a request or notification.
    pub fn params(&self) -> Option<&Value> {
        match self {
            JsonRpcMessage::Request { params, .. } => params.as_ref(),
            JsonRpcMessage::Notification { params, .. } => params.as_ref(),
            _ => None,
        }
    }

    /// Returns true if this message is a notification (no response expected).
    pub fn is_notification(&self) -> bool {
        matches!(self, JsonRpcMessage::Notification { .. })
    }

    /// Returns true if this message is a batch.
    pub fn is_batch(&self) -> bool {
        matches!(self, JsonRpcMessage::Batch(_))
    }

    /// Create a method not found error response for the given request.
    pub fn method_not_found_response(id: RequestId, method: &str) -> Self {
        JsonRpcMessage::Error {
            id,
            error: JsonRpcErrorObj::new(METHOD_NOT_FOUND, format!("Method not found: {}", method)),
        }
    }

    /// Create a success response for the given request ID and result.
    pub fn success_response(id: RequestId, result: Value) -> Self {
        JsonRpcMessage::Success { id, result }
    }

    /// Create an error response for the given request ID.
    pub fn error_response(id: RequestId, code: i64, message: impl Into<String>) -> Self {
        JsonRpcMessage::Error {
            id,
            error: JsonRpcErrorObj::new(code, message),
        }
    }

    /// Create an error response with data.
    pub fn error_response_with_data(
        id: RequestId,
        code: i64,
        message: impl Into<String>,
        data: Value,
    ) -> Self {
        JsonRpcMessage::Error {
            id,
            error: JsonRpcErrorObj::with_data(code, message, data),
        }
    }

    /// Create a parse error response (used when the original message couldn't be parsed).
    pub fn parse_error_response() -> Self {
        JsonRpcMessage::Error {
            id: RequestId::Null,
            error: JsonRpcErrorObj::new(PARSE_ERROR, "Parse error"),
        }
    }

    /// Create an invalid request error response.
    pub fn invalid_request_response() -> Self {
        JsonRpcMessage::Error {
            id: RequestId::Null,
            error: JsonRpcErrorObj::new(INVALID_REQUEST, "Invalid Request"),
        }
    }

    /// Flatten batch messages into individual responses for error handling.
    pub fn flatten(self) -> Vec<JsonRpcMessage> {
        match self {
            JsonRpcMessage::Batch(msgs) => msgs,
            msg => vec![msg],
        }
    }
}

use serde_json::json;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_request_with_string_id() {
        let msg = JsonRpcMessage::from_str(
            r#"{"jsonrpc":"2.0","id":"abc","method":"tools/list","params":{}}"#,
        )
        .unwrap();
        match msg {
            JsonRpcMessage::Request { id, method, params } => {
                assert_eq!(id, RequestId::String("abc".into()));
                assert_eq!(method, "tools/list");
                assert!(params.is_some());
            }
            _ => panic!("Expected Request"),
        }
    }

    #[test]
    fn test_parse_request_with_integer_id() {
        let msg = JsonRpcMessage::from_str(
            r#"{"jsonrpc":"2.0","id":42,"method":"ping"}"#,
        )
        .unwrap();
        match msg {
            JsonRpcMessage::Request { id, method, params } => {
                assert_eq!(id, RequestId::Integer(42));
                assert_eq!(method, "ping");
                assert!(params.is_none());
            }
            _ => panic!("Expected Request"),
        }
    }

    #[test]
    fn test_parse_notification() {
        let msg = JsonRpcMessage::from_str(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        )
        .unwrap();
        match msg {
            JsonRpcMessage::Notification { method, params } => {
                assert_eq!(method, "notifications/initialized");
                assert!(params.is_none());
            }
            _ => panic!("Expected Notification"),
        }
    }

    #[test]
    fn test_parse_success_response() {
        let msg = JsonRpcMessage::from_str(
            r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#,
        )
        .unwrap();
        match msg {
            JsonRpcMessage::Success { id, result } => {
                assert_eq!(id, RequestId::Integer(1));
                assert_eq!(result["tools"], json!([]));
            }
            _ => panic!("Expected Success"),
        }
    }

    #[test]
    fn test_parse_error_response() {
        let msg = JsonRpcMessage::from_str(
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}"#,
        )
        .unwrap();
        match msg {
            JsonRpcMessage::Error { id, error } => {
                assert_eq!(id, RequestId::Integer(1));
                assert_eq!(error.code, -32601);
            }
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn test_parse_batch() {
        let msg = JsonRpcMessage::from_str(
            r#"[{"jsonrpc":"2.0","id":1,"method":"tools/list"},{"jsonrpc":"2.0","id":2,"method":"ping"}]"#,
        )
        .unwrap();
        match msg {
            JsonRpcMessage::Batch(msgs) => {
                assert_eq!(msgs.len(), 2);
            }
            _ => panic!("Expected Batch"),
        }
    }

    #[test]
    fn test_reject_null_id_in_request() {
        let result = JsonRpcMessage::from_str(
            r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#,
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            McpError::InvalidRequest(msg) => assert!(msg.contains("null")),
            _ => panic!("Expected InvalidRequest"),
        }
    }

    #[test]
    fn test_reject_invalid_jsonrpc_version() {
        let result = JsonRpcMessage::from_str(
            r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_reject_empty_batch() {
        let result = JsonRpcMessage::from_str(r#"[]"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_reject_malformed_json() {
        let result = JsonRpcMessage::from_str(r#"{not json}"#);
        assert!(result.is_err());
        match result.unwrap_err() {
            McpError::ParseError(_) => {}
            _ => panic!("Expected ParseError"),
        }
    }

    #[test]
    fn test_roundtrip_request() {
        let original = JsonRpcMessage::Request {
            id: RequestId::Integer(1),
            method: "tools/call".to_string(),
            params: Some(json!({"name": "echo"})),
        };
        let serialized = original.to_json_string();
        let deserialized = JsonRpcMessage::from_str(&serialized).unwrap();
        match deserialized {
            JsonRpcMessage::Request { id, method, params } => {
                assert_eq!(id, RequestId::Integer(1));
                assert_eq!(method, "tools/call");
                assert_eq!(params.unwrap()["name"], "echo");
            }
            _ => panic!("Expected Request after roundtrip"),
        }
    }

    #[test]
    fn test_roundtrip_notification() {
        let original = JsonRpcMessage::Notification {
            method: "notifications/initialized".to_string(),
            params: None,
        };
        let serialized = original.to_json_string();
        let deserialized = JsonRpcMessage::from_str(&serialized).unwrap();
        assert!(deserialized.is_notification());
        assert_eq!(deserialized.method(), Some("notifications/initialized"));
    }

    #[test]
    fn test_reject_non_object_message() {
        let result = JsonRpcMessage::from_str(r#""hello""#);
        assert!(result.is_err());
        match result.unwrap_err() {
            McpError::InvalidRequest(_) => {}
            _ => panic!("Expected InvalidRequest"),
        }
    }

    #[test]
    fn test_method_not_found_response() {
        let resp = JsonRpcMessage::method_not_found_response(RequestId::Integer(1), "foo/bar");
        match resp {
            JsonRpcMessage::Error { id, error } => {
                assert_eq!(id, RequestId::Integer(1));
                assert_eq!(error.code, METHOD_NOT_FOUND);
            }
            _ => panic!("Expected Error"),
        }
    }

    #[test]
    fn test_display_trait() {
        let msg = JsonRpcMessage::Request {
            id: RequestId::Integer(42),
            method: "tools/call".to_string(),
            params: None,
        };
        assert_eq!(format!("{}", msg), "Request(id=Integer(42), method=tools/call)");
    }
}
