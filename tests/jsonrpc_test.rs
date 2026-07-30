//! Integration tests for JSON-RPC parsing and serialization.

use mcp_shield::error::RequestId;
use mcp_shield::protocol::jsonrpc::{
    INVALID_REQUEST, JsonRpcMessage, METHOD_NOT_FOUND, PARSE_ERROR,
};
use serde_json::json;

#[test]
fn test_full_request_lifecycle() {
    // Client sends initialize request
    let init_request = JsonRpcMessage::from_str(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#,
    )
    .unwrap();

    assert!(matches!(init_request, JsonRpcMessage::Request { .. }));
    assert_eq!(init_request.method(), Some("initialize"));
}

#[test]
fn test_batch_message_handling() {
    let batch = JsonRpcMessage::from_str(
        r#"[
            {"jsonrpc":"2.0","id":1,"method":"tools/list"},
            {"jsonrpc":"2.0","id":2,"method":"ping"},
            {"jsonrpc":"2.0","method":"notifications/initialized"}
        ]"#,
    )
    .unwrap();

    match batch {
        JsonRpcMessage::Batch(msgs) => {
            assert_eq!(msgs.len(), 3);
            // First two are requests
            assert!(matches!(msgs[0], JsonRpcMessage::Request { .. }));
            assert!(matches!(msgs[1], JsonRpcMessage::Request { .. }));
            // Third is a notification
            assert!(matches!(msgs[2], JsonRpcMessage::Notification { .. }));
        }
        _ => panic!("Expected batch"),
    }
}

#[test]
fn test_error_response_construction() {
    let error = JsonRpcMessage::error_response(
        RequestId::Integer(42),
        METHOD_NOT_FOUND,
        "tools/unknown not found",
    );

    let json = error.to_value();
    assert_eq!(json["jsonrpc"], "2.0");
    assert_eq!(json["id"], 42);
    assert_eq!(json["error"]["code"], METHOD_NOT_FOUND);
}

#[test]
fn test_notification_has_no_id() {
    let notification = JsonRpcMessage::Notification {
        method: "notifications/initialized".to_string(),
        params: None,
    };

    let json = notification.to_value();
    assert!(json.get("id").is_none());
    assert_eq!(json["method"], "notifications/initialized");
}

#[test]
fn test_malformed_json_returns_parse_error() {
    let result = JsonRpcMessage::from_str(r#"{broken json}"#);
    assert!(result.is_err());

    let err = result.unwrap_err();
    assert_eq!(err.code(), PARSE_ERROR);
}

#[test]
fn test_id_type_preservation() {
    // String ID
    let msg = JsonRpcMessage::from_str(r#"{"jsonrpc":"2.0","id":"req-abc-123","method":"ping"}"#)
        .unwrap();
    assert!(matches!(
        msg.id(),
        Some(RequestId::String(s)) if s == "req-abc-123"
    ));

    // Integer ID
    let msg = JsonRpcMessage::from_str(r#"{"jsonrpc":"2.0","id":99999,"method":"ping"}"#).unwrap();
    assert!(matches!(
        msg.id(),
        Some(RequestId::Integer(n)) if *n == 99999
    ));
}
