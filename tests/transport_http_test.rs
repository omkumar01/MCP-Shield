//! Integration tests for the transport layer.

use mcp_shield::error::RequestId;
use mcp_shield::protocol::jsonrpc::JsonRpcMessage;
use serde_json::json;

#[test]
fn test_stdio_message_roundtrip() {
    // Simulate a message that would come over stdio
    let input = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#;

    let message = JsonRpcMessage::from_str(input).unwrap();
    assert_eq!(message.method(), Some("initialize"));

    // Serialize back
    let output = message.to_json_string();
    let reparsed = JsonRpcMessage::from_str(&output).unwrap();
    assert_eq!(reparsed.method(), Some("initialize"));
}

#[test]
fn test_http_post_message_format() {
    // Verify a message can be parsed from an HTTP POST body
    let body = r#"{"jsonrpc":"2.0","id":42,"method":"tools/call","params":{"name":"com.echo.echo","arguments":{"message":"hello"}}}"#;

    let message = JsonRpcMessage::from_str(body).unwrap();
    match message {
        JsonRpcMessage::Request { id, method, params } => {
            assert_eq!(id, RequestId::Integer(42));
            assert_eq!(method, "tools/call");
            assert_eq!(params.unwrap()["name"], "com.echo.echo");
        }
        _ => panic!("Expected Request"),
    }
}

#[test]
fn test_sse_event_serialization() {
    // A response that would be sent as an SSE event
    let response = JsonRpcMessage::success_response(
        RequestId::Integer(1),
        json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {"tools": {"listChanged": true}},
            "serverInfo": {"name": "mcp-shield", "version": "0.1.0"}
        }),
    );

    let event_data = response.to_json_string();
    let reparsed = JsonRpcMessage::from_str(&event_data).unwrap();
    assert!(matches!(reparsed, JsonRpcMessage::Success { .. }));
}

#[test]
fn test_batch_processing() {
    let batch_input = r#"[
        {"jsonrpc":"2.0","id":1,"method":"tools/list"},
        {"jsonrpc":"2.0","id":2,"method":"ping"},
        {"jsonrpc":"2.0","method":"notifications/initialized"}
    ]"#;

    let batch = JsonRpcMessage::from_str(batch_input).unwrap();
    let messages = batch.flatten();
    assert_eq!(messages.len(), 3);
}

#[test]
fn test_notification_no_response() {
    let notification = JsonRpcMessage::Notification {
        method: "notifications/initialized".to_string(),
        params: None,
    };

    // Notifications should not have an ID
    assert!(notification.id().is_none());
    assert!(notification.is_notification());
}
