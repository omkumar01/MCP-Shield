//! Integration tests for JSON Schema 2020-12 validation.

use mcp_shield::protocol::schema::{SchemaValidator, validate_once};
use serde_json::json;

#[tokio::test]
async fn test_validate_echo_tool_schema() {
    let schema = json!({
        "type": "object",
        "properties": {
            "message": {"type": "string", "minLength": 1}
        },
        "required": ["message"]
    });

    let mut validator = SchemaValidator::new();

    // Valid
    assert!(
        validator
            .validate(&schema, &json!({"message": "hello"}))
            .await
            .is_ok()
    );

    // Missing required field
    assert!(validator.validate(&schema, &json!({})).await.is_err());

    // Wrong type
    assert!(
        validator
            .validate(&schema, &json!({"message": 123}))
            .await
            .is_err()
    );

    // Empty string violates minLength
    assert!(
        validator
            .validate(&schema, &json!({"message": ""}))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn test_validate_add_tool_schema() {
    let schema = json!({
        "type": "object",
        "properties": {
            "a": {"type": "integer", "minimum": 0},
            "b": {"type": "integer", "minimum": 0}
        },
        "required": ["a", "b"]
    });

    let mut validator = SchemaValidator::new();

    assert!(
        validator
            .validate(&schema, &json!({"a": 1, "b": 2}))
            .await
            .is_ok()
    );
    assert!(
        validator
            .validate(&schema, &json!({"a": -1, "b": 2}))
            .await
            .is_err()
    );
    assert!(validator.validate(&schema, &json!({"a": 1})).await.is_err());
}

#[tokio::test]
async fn test_schema_with_additional_properties() {
    let schema = json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"}
        },
        "additionalProperties": false
    });

    let mut validator = SchemaValidator::new();

    // Additional property should fail
    assert!(
        validator
            .validate(&schema, &json!({"name": "test", "extra": true}))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn test_validator_cache_persistence() {
    let schema = json!({"type": "string"});

    let mut validator = SchemaValidator::new();
    assert_eq!(validator.cache_size(), 0);

    // First validation compiles and caches
    validator.validate(&schema, &json!("test")).await.unwrap();
    assert_eq!(validator.cache_size(), 1);

    // Multiple validations don't increase cache size
    for _ in 0..10 {
        validator.validate(&schema, &json!("test")).await.unwrap();
    }
    assert_eq!(validator.cache_size(), 1);
}

#[tokio::test]
async fn test_validate_once() {
    let schema = json!({"type": "string", "minLength": 1});
    assert!(validate_once(&schema, &json!("hello")).await.is_ok());
    assert!(validate_once(&schema, &json!("")).await.is_err());
}
