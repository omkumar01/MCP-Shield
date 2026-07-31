//! Phase 2 integration tests — Cedar authorizer, session locking, audit producer.

use mcp_shield::{
    auth::scope::{SCOPE_ADMIN, SCOPE_TOOLS_CALL, SCOPE_TOOLS_READ, ScopeEnforcer},
    gateway::{McpRouter, ToolRegistry, UpstreamProxy},
    policy::{CedarAuthorizer, CedarPolicyAuthorizer},
    session::state::{ContextScope, InMemorySessionManager, SessionManager, Visibility},
    telemetry::metrics::McpMetrics,
    telemetry::producer::{AuditEvent, AuthDecision, BufferingProducer, EventProducer},
};
use std::collections::HashMap;
use std::sync::Arc;

const TEST_POLICY: &str = r#"
permit (
    principal,
    action == Action::"tools/list",
    resource
);

permit (
    principal,
    action == Action::"tools/call",
    resource == Tool::"com.echo:echo"
)
when {
    principal has scopes && principal.scopes.contains("mcp:tools:call")
};

permit (
    principal,
    action == Action::"shutdown",
    resource
)
when {
    principal has scopes && principal.scopes.contains("mcp:admin")
};
"#;

async fn make_test_router() -> Arc<McpRouter> {
    let registry = Arc::new(ToolRegistry::new());

    // Register echo server tools in the registry
    for tool in mcp_shield::test_server::EchoServer::list_tools() {
        registry.register_tool(tool, "echo").await.unwrap();
    }

    let proxy = Arc::new(UpstreamProxy::new(10, 10));
    let metrics = Arc::new(McpMetrics::new());

    // Register the echo server as an upstream (needed for tools/call to work)
    proxy
        .register_server(mcp_shield::gateway::proxy::UpstreamServer {
            id: "echo".to_string(),
            transport: mcp_shield::gateway::proxy::UpstreamTransport::StreamableHttp,
            url: None,
            is_echo: true,
        })
        .await;

    let authorizer = Some(
        Arc::new(CedarPolicyAuthorizer::new(TEST_POLICY.to_string()).unwrap())
            as Arc<dyn CedarAuthorizer>,
    );
    let sessions = Some(Arc::new(InMemorySessionManager::new()) as Arc<dyn SessionManager>);

    Arc::new(McpRouter::with_policy(
        registry,
        proxy,
        metrics,
        "test-server".to_string(),
        "0.1.0".to_string(),
        authorizer,
        sessions,
    ))
}

#[tokio::test]
async fn test_cedar_integration_tools_list_allowed() {
    let router = make_test_router().await;
    let enforcer = ScopeEnforcer::new(vec![SCOPE_TOOLS_READ.to_string()]);

    let request = mcp_shield::protocol::jsonrpc::JsonRpcMessage::Request {
        id: mcp_shield::error::RequestId::Integer(1),
        method: "tools/list".to_string(),
        params: None,
    };

    let response = router
        .handle_message(request, &enforcer, Some("test-session"))
        .await;
    if let Err(ref e) = response {
        eprintln!("Error: {:?}", e);
    }
    assert!(response.is_ok());
    let resp = response.unwrap();
    assert!(matches!(
        resp,
        mcp_shield::protocol::jsonrpc::JsonRpcMessage::Success { .. }
    ));
}

#[tokio::test]
async fn test_cedar_integration_tools_call_allowed_with_scope() {
    let router = make_test_router().await;
    let enforcer = ScopeEnforcer::new(vec![
        SCOPE_TOOLS_READ.to_string(),
        SCOPE_TOOLS_CALL.to_string(),
    ]);

    let request = mcp_shield::protocol::jsonrpc::JsonRpcMessage::Request {
        id: mcp_shield::error::RequestId::Integer(2),
        method: "tools/call".to_string(),
        params: Some(serde_json::json!({
            "name": "com.echo:echo",
            "arguments": {"message": "hello"}
        })),
    };

    let response = router
        .handle_message(request, &enforcer, Some("test-session"))
        .await;
    if let Err(ref e) = response {
        eprintln!("Error: {:?}", e);
    }
    // Should be allowed by Cedar with the right scope
    assert!(response.is_ok());
}

#[tokio::test]
async fn test_cedar_integration_tools_call_denied_without_scope() {
    let router = make_test_router().await;
    let enforcer = ScopeEnforcer::new(vec![SCOPE_TOOLS_READ.to_string()]);

    let request = mcp_shield::protocol::jsonrpc::JsonRpcMessage::Request {
        id: mcp_shield::error::RequestId::Integer(3),
        method: "tools/call".to_string(),
        params: Some(serde_json::json!({
            "name": "com.echo.echo",
            "arguments": {"message": "hello"}
        })),
    };

    let response = router
        .handle_message(request, &enforcer, Some("test-session"))
        .await;
    // Should be denied by Cedar without the right scope
    assert!(response.is_err());
    let err = response.unwrap_err();
    assert_eq!(err.code(), -32002); // Cedar deny
}

#[tokio::test]
async fn test_cedar_integration_shutdown_allowed_with_admin() {
    let router = make_test_router().await;
    let enforcer = ScopeEnforcer::new(vec![SCOPE_ADMIN.to_string()]);

    let request = mcp_shield::protocol::jsonrpc::JsonRpcMessage::Request {
        id: mcp_shield::error::RequestId::Integer(4),
        method: "shutdown".to_string(),
        params: None,
    };

    let response = router
        .handle_message(request, &enforcer, Some("test-session"))
        .await;
    assert!(response.is_ok());
}

#[tokio::test]
async fn test_cedar_integration_shutdown_denied_without_admin() {
    let router = make_test_router().await;
    let enforcer = ScopeEnforcer::new(vec![SCOPE_TOOLS_CALL.to_string()]);

    let request = mcp_shield::protocol::jsonrpc::JsonRpcMessage::Request {
        id: mcp_shield::error::RequestId::Integer(5),
        method: "shutdown".to_string(),
        params: None,
    };

    let response = router
        .handle_message(request, &enforcer, Some("test-session"))
        .await;
    assert!(response.is_err());
    let err = response.unwrap_err();
    assert_eq!(err.code(), -32002);
}

#[tokio::test]
async fn test_session_locking_blocks_cross_context() {
    let router = make_test_router();
    let enforcer = ScopeEnforcer::permissive();
    let session_mgr = InMemorySessionManager::new();
    let session = session_mgr
        .create_session(Some("client-1".into()))
        .await
        .unwrap();

    // Lock session to a public context
    let public_ctx = ContextScope {
        context_type: "github_repo".to_string(),
        identifier: "owner/public-repo".to_string(),
        visibility: Visibility::Public,
    };
    session_mgr
        .lock_context(&session.session_id, public_ctx)
        .await
        .unwrap();

    // Try to access a private context - should be blocked
    let private_ctx = ContextScope {
        context_type: "github_repo".to_string(),
        identifier: "owner/private-repo".to_string(),
        visibility: Visibility::Private,
    };

    let result = session_mgr
        .check_context_access(&session.session_id, &private_ctx)
        .await
        .unwrap();
    assert!(matches!(
        result,
        mcp_shield::session::state::AccessCheckResult::Deny { .. }
    ));
}

#[tokio::test]
async fn test_session_locking_allows_same_context() {
    let session_mgr = InMemorySessionManager::new();
    let session = session_mgr.create_session(None).await.unwrap();

    let ctx = ContextScope {
        context_type: "filesystem".to_string(),
        identifier: "/workspace/project".to_string(),
        visibility: Visibility::Private,
    };
    session_mgr
        .lock_context(&session.session_id, ctx.clone())
        .await
        .unwrap();

    let result = session_mgr
        .check_context_access(&session.session_id, &ctx)
        .await
        .unwrap();
    assert!(matches!(
        result,
        mcp_shield::session::state::AccessCheckResult::Allow
    ));
}

#[tokio::test]
async fn test_session_logging() {
    let session_mgr = InMemorySessionManager::new();
    let session = session_mgr.create_session(None).await.unwrap();

    let record = mcp_shield::session::state::ToolCallRecord {
        tool_name: "com.example:test".to_string(),
        arguments: serde_json::json!({"key": "value"}),
        timestamp: chrono::Utc::now(),
    };

    session_mgr
        .log_tool_call(&session.session_id, record)
        .await
        .unwrap();

    let retrieved = session_mgr
        .get_session(&session.session_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retrieved.tool_calls.len(), 1);
    assert_eq!(retrieved.tool_calls[0].tool_name, "com.example:test");
}

#[tokio::test]
async fn test_buffering_producer_captures_events() {
    let producer = BufferingProducer::new();
    let event = AuditEvent {
        event_id: "test-1".to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        method: "tools/call".to_string(),
        request_id: Some("req-1".to_string()),
        session_id: Some("sess-1".to_string()),
        principal: Some("client-1".to_string()),
        scopes: vec!["mcp:tools:call".to_string()],
        decision: AuthDecision::Allow,
        request_payload: None,
        response_payload: None,
        error_code: None,
        duration_ms: 10,
        transport: "http".to_string(),
    };

    producer.publish_audit_event(event.clone()).await.unwrap();
    assert_eq!(producer.len().await, 1);

    let drained = producer.drain().await;
    assert_eq!(drained.len(), 1);
    assert_eq!(drained[0].decision, AuthDecision::Allow);
}

#[tokio::test]
async fn test_audit_event_decision_mapping() {
    let producer = BufferingProducer::new();

    // Test Allow
    let event_allow = AuditEvent {
        event_id: "1".to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        method: "tools/list".to_string(),
        request_id: Some("req-1".to_string()),
        session_id: Some("sess-1".to_string()),
        principal: Some("client-1".to_string()),
        scopes: vec!["mcp:tools:read".to_string()],
        decision: AuthDecision::Allow,
        request_payload: None,
        response_payload: None,
        error_code: None,
        duration_ms: 5,
        transport: "http".to_string(),
    };
    producer.publish_audit_event(event_allow).await.unwrap();

    // Test Deny
    let event_deny = AuditEvent {
        event_id: "2".to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        method: "tools/call".to_string(),
        request_id: Some("req-2".to_string()),
        session_id: Some("sess-2".to_string()),
        principal: Some("client-2".to_string()),
        scopes: vec!["mcp:tools:read".to_string()],
        decision: AuthDecision::Deny,
        request_payload: None,
        response_payload: None,
        error_code: Some(-32002),
        duration_ms: 3,
        transport: "http".to_string(),
    };
    producer.publish_audit_event(event_deny).await.unwrap();

    let events = producer.drain().await;
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].decision, AuthDecision::Allow);
    assert_eq!(events[1].decision, AuthDecision::Deny);
}

#[tokio::test]
async fn test_end_to_end_with_audit() {
    let producer = Arc::new(BufferingProducer::new());
    let registry = Arc::new(ToolRegistry::new());

    // Register echo server tools
    for tool in mcp_shield::test_server::EchoServer::list_tools() {
        registry.register_tool(tool, "echo").await.unwrap();
    }

    let proxy = Arc::new(UpstreamProxy::new(10, 10));
    let metrics = Arc::new(McpMetrics::new());

    // Register echo server
    proxy
        .register_server(mcp_shield::gateway::proxy::UpstreamServer {
            id: "echo".to_string(),
            transport: mcp_shield::gateway::proxy::UpstreamTransport::StreamableHttp,
            url: None,
            is_echo: true,
        })
        .await;

    let authorizer = Some(
        Arc::new(CedarPolicyAuthorizer::new(TEST_POLICY.to_string()).unwrap())
            as Arc<dyn CedarAuthorizer>,
    );
    let sessions = Some(Arc::new(InMemorySessionManager::new()) as Arc<dyn SessionManager>);

    let router = Arc::new(McpRouter::with_full_config(
        registry,
        proxy,
        metrics,
        "test-server".to_string(),
        "0.1.0".to_string(),
        authorizer,
        sessions,
        Some(producer.clone()),
    ));

    let enforcer = ScopeEnforcer::new(vec![
        SCOPE_TOOLS_READ.to_string(),
        SCOPE_TOOLS_CALL.to_string(),
    ]);

    let request = mcp_shield::protocol::jsonrpc::JsonRpcMessage::Request {
        id: mcp_shield::error::RequestId::Integer(10),
        method: "tools/call".to_string(),
        params: Some(serde_json::json!({
            "name": "com.echo:echo",
            "arguments": {"message": "test"}
        })),
    };

    let response = router
        .handle_message(request, &enforcer, Some("audit-session"))
        .await;
    assert!(response.is_ok());

    // Give some time for the audit event to be published
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let events = producer.drain().await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].method, "tools/call");
    assert_eq!(events[0].decision, AuthDecision::Allow);
    assert_eq!(events[0].session_id, Some("audit-session".to_string()));
}
