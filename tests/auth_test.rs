//! Integration tests for authentication and scope enforcement.

use mcp_shield::auth::jwt::{JwtValidator, JwtValidatorConfig};
use mcp_shield::auth::scope::{ScopeEnforcer, SCOPE_TOOLS_CALL, SCOPE_TOOLS_READ, SCOPE_ADMIN};
use jsonwebtoken::{encode, EncodingKey, Header};
use serde_json::json;

fn create_test_jwt(secret: &str, claims: serde_json::Value) -> String {
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap()
}

#[test]
fn test_jwt_validation_lifecycle() {
    let validator = JwtValidator::with_hmac_secret("test-secret", Some("mcp-shield".into()));

    let claims = json!({
        "sub": "client-test",
        "iss": "mcp-shield",
        "scope": "mcp:tools:read mcp:tools:call",
        "exp": (chrono::Utc::now().timestamp() + 3600) as u64
    });

    let token = create_test_jwt("test-secret", claims);
    let validated = validator.validate_token(&token).unwrap();

    assert_eq!(validated.sub, Some("client-test".into()));
    assert_eq!(validated.iss, Some("mcp-shield".into()));
    assert!(validated.scope.is_some());
}

#[test]
fn test_scope_enforcement_read_only() {
    let enforcer = ScopeEnforcer::new(vec![SCOPE_TOOLS_READ.to_string()]);

    // Can list tools
    assert!(enforcer.check_method("tools/list").is_ok());

    // Cannot call tools without the call scope
    assert!(enforcer.check_method("tools/call").is_err());
}

#[test]
fn test_scope_enforcement_per_tool() {
    let enforcer = ScopeEnforcer::new(vec![
        format!("{}:com.example", SCOPE_TOOLS_CALL),
    ]);

    // Can call tools in the allowed prefix
    assert!(enforcer.check_tool_access("com.example:echo").is_ok());

    // Cannot call tools in a different prefix
    assert!(enforcer.check_tool_access("com.other:search").is_err());
}

#[test]
fn test_scope_enforcement_admin() {
    let enforcer = ScopeEnforcer::new(vec![SCOPE_ADMIN.to_string()]);

    // Admin can do everything
    assert!(enforcer.check_method("tools/call").is_ok());
    assert!(enforcer.check_method("shutdown").is_ok());
    assert!(enforcer.check_tool_access("com.anything:tool").is_ok());
}

#[test]
fn test_expired_token_rejected() {
    let validator = JwtValidator::with_hmac_secret("secret", None);

    let claims = json!({
        "sub": "client",
        "exp": (chrono::Utc::now().timestamp() - 100) as u64  // Expired
    });

    let token = create_test_jwt("secret", claims);
    assert!(validator.validate_token(&token).is_err());
}

#[test]
fn test_bearer_token_extraction() {
    assert_eq!(
        JwtValidator::extract_bearer_token("Bearer abc123").unwrap(),
        "abc123"
    );

    assert!(JwtValidator::extract_bearer_token("Token abc").is_err());
    assert!(JwtValidator::extract_bearer_token("Bearer").is_err());
}
