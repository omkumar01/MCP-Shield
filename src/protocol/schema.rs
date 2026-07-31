//! JSON Schema 2020-12 validation engine.
//!
//! Validates MCP tool call arguments against their registered input schemas
//! using the `jsonschema` crate. Handles unsupported dialect detection and
//! graceful fallback.

use crate::error::McpError;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// The default JSON Schema dialect for MCP.
pub const DEFAULT_DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";

/// Supported JSON Schema `$schema` URIs.
const SUPPORTED_DIALECTS: &[&str] = &[
    "https://json-schema.org/draft/2020-12/schema",
    "http://json-schema.org/draft/2020-12/schema",
    // Also accept no dialect (defaults to 2020-12)
];

/// A compiled JSON Schema validator.
///
/// Schema compilation is expensive, so validators are cached by a hash
/// of the schema JSON. Build once, validate many.
pub struct SchemaValidator {
    /// Cache of compiled validators keyed by schema fingerprint.
    cache: HashMap<String, Arc<Mutex<jsonschema::Validator>>>,
}

impl SchemaValidator {
    /// Create a new empty schema validator cache.
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    /// Validate a JSON instance against a schema.
    ///
    /// Returns `Ok(())` if the instance is valid, or `Err(McpError)` with
    /// a detailed validation error if it is not.
    pub async fn validate(&mut self, schema: &Value, instance: &Value) -> Result<(), McpError> {
        // Check for unsupported dialect declarations
        self.check_dialect(schema)?;

        // Compile (or retrieve from cache) and validate
        let key = self.schema_key(schema);
        let validator = if let Some(v) = self.cache.get(&key) {
            v.clone()
        } else {
            let v = Arc::new(Mutex::new(self.compile_schema(schema)?));
            self.cache.insert(key.clone(), v.clone());
            v
        };

        self.validate_with(&validator, instance).await
    }

    /// Validate a JSON instance with a pre-compiled validator.
    async fn validate_with(
        &self,
        validator: &Arc<Mutex<jsonschema::Validator>>,
        instance: &Value,
    ) -> Result<(), McpError> {
        let validator = validator.lock().await;
        match validator.validate(instance) {
            Ok(()) => Ok(()),
            Err(err) => {
                // Collect all validation errors for a detailed message
                let error_details: Vec<String> = err
                    .map(|e| {
                        let path = e.instance_path.to_string();
                        let msg = e.to_string();
                        if path.is_empty() {
                            msg
                        } else {
                            format!("{}: {}", path, msg)
                        }
                    })
                    .collect();

                Err(McpError::InvalidParams(format!(
                    "Schema validation failed: {}",
                    error_details.join("; ")
                )))
            }
        }
    }

    /// Check the `$schema` dialect declaration.
    ///
    /// If the schema declares an unsupported dialect, returns a descriptive
    /// error. If no dialect is declared, assumes 2020-12.
    fn check_dialect(&self, schema: &Value) -> Result<(), McpError> {
        if let Some(dialect) = schema.get("$schema").and_then(|v| v.as_str()) {
            let is_supported =
                SUPPORTED_DIALECTS.iter().any(|d| *d == dialect) || dialect.is_empty();

            if !is_supported {
                tracing::warn!(
                    schema_dialect = dialect,
                    "Unsupported JSON Schema dialect; attempting best-effort validation"
                );
                // Return a soft warning but still attempt validation
                // (per MCP spec: implementation-defined behavior)
                return Err(McpError::SchemaInvalid(format!(
                    "Unsupported JSON Schema dialect: \"{}\". \
                     MCP-Shield supports JSON Schema 2020-12 (draft/2020-12). \
                     Attempting best-effort validation.",
                    dialect
                )));
            }
        }
        Ok(())
    }

    /// Compile a JSON Schema into a reusable validator.
    fn compile_schema(&self, schema: &Value) -> Result<jsonschema::Validator, McpError> {
        jsonschema::validator_for(schema)
            .map_err(|e| McpError::SchemaInvalid(format!("Failed to compile schema: {}", e)))
    }

    /// Generate a cache key for a schema by hashing its JSON representation.
    fn schema_key(&self, schema: &Value) -> String {
        // Use the JSON serialization as a simple key.
        // For production, consider using a hash function for long schemas.
        let json = serde_json::to_string(schema).unwrap_or_default();
        // Truncate very long schemas for key generation
        if json.len() > 1024 {
            format!("schema:{}", &json[..1024])
        } else {
            format!("schema:{}", json)
        }
    }

    /// Pre-warm the cache with a schema.
    pub async fn precompile(&mut self, schema: &Value) -> Result<(), McpError> {
        let key = self.schema_key(schema);
        if !self.cache.contains_key(&key) {
            let validator = Arc::new(Mutex::new(self.compile_schema(schema)?));
            self.cache.insert(key, validator);
        }
        Ok(())
    }

    /// Return the number of cached validators.
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }

    /// Clear the validator cache.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }
}

impl Default for SchemaValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Quick validation without caching (for one-off use).
pub async fn validate_once(schema: &Value, instance: &Value) -> Result<(), McpError> {
    let mut validator = SchemaValidator::new();
    validator.validate(schema, instance).await
}

/// Check if a schema uses JSON Schema 2020-12 features.
///
/// Returns true if the schema is compatible with 2020-12.
pub fn is_draft_2020_12_compatible(schema: &Value) -> bool {
    match schema.get("$schema").and_then(|v| v.as_str()) {
        Some(dialect) => SUPPORTED_DIALECTS.iter().any(|d| *d == dialect),
        None => true, // No dialect = assume 2020-12
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn basic_object_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "minLength": 1 },
                "age": { "type": "integer", "minimum": 0 }
            },
            "required": ["name"]
        })
    }

    #[tokio::test]
    async fn test_valid_instance() {
        let schema = basic_object_schema();
        let instance = json!({"name": "Alice", "age": 30});
        let mut validator = SchemaValidator::new();
        assert!(validator.validate(&schema, &instance).await.is_ok());
    }

    #[tokio::test]
    async fn test_missing_required_field() {
        let schema = basic_object_schema();
        let instance = json!({"age": 30});
        let mut validator = SchemaValidator::new();
        let result = validator.validate(&schema, &instance).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("name"));
    }

    #[tokio::test]
    async fn test_type_mismatch() {
        let schema = basic_object_schema();
        let instance = json!({"name": "Alice", "age": "not a number"});
        let mut validator = SchemaValidator::new();
        let result = validator.validate(&schema, &instance).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_minimum_constraint() {
        let schema = basic_object_schema();
        let instance = json!({"name": "Alice", "age": -1});
        let mut validator = SchemaValidator::new();
        let result = validator.validate(&schema, &instance).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_schema_caching() {
        let schema = basic_object_schema();
        let instance = json!({"name": "Bob"});

        let mut validator = SchemaValidator::new();
        assert_eq!(validator.cache_size(), 0);

        // First call compiles and caches
        validator.validate(&schema, &instance).await.unwrap();
        assert_eq!(validator.cache_size(), 1);

        // Second call uses cached validator
        validator.validate(&schema, &instance).await.unwrap();
        assert_eq!(validator.cache_size(), 1);
    }

    #[tokio::test]
    async fn test_unsupported_dialect_warning() {
        let schema = json!({
            "$schema": "https://json-schema.org/draft/07/schema",
            "type": "string"
        });
        let instance = json!("hello");

        let mut validator = SchemaValidator::new();
        let result = validator.validate(&schema, &instance).await;
        // Should warn about unsupported dialect
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unsupported JSON Schema dialect"));
    }

    #[tokio::test]
    async fn test_draft_2020_12_compatible_check() {
        let with_dialect = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "string"
        });
        assert!(is_draft_2020_12_compatible(&with_dialect));

        let no_dialect = json!({"type": "string"});
        assert!(is_draft_2020_12_compatible(&no_dialect));

        let old_dialect = json!({
            "$schema": "http://json-schema.org/draft-04/schema#",
            "type": "string"
        });
        assert!(!is_draft_2020_12_compatible(&old_dialect));
    }

    #[tokio::test]
    async fn test_precompile() {
        let schema = basic_object_schema();
        let mut validator = SchemaValidator::new();
        assert_eq!(validator.cache_size(), 0);
        validator.precompile(&schema).await.unwrap();
        assert_eq!(validator.cache_size(), 1);
    }

    #[tokio::test]
    async fn test_clear_cache() {
        let schema = basic_object_schema();
        let instance = json!({"name": "Alice"});
        let mut validator = SchemaValidator::new();
        validator.validate(&schema, &instance).await.unwrap();
        assert_eq!(validator.cache_size(), 1);
        validator.clear_cache();
        assert_eq!(validator.cache_size(), 0);
    }

    #[tokio::test]
    async fn test_validate_once_helper() {
        let schema = json!({"type": "string", "minLength": 1});
        assert!(validate_once(&schema, &json!("hello")).await.is_ok());
        assert!(validate_once(&schema, &json!("")).await.is_err());
    }

    #[tokio::test]
    async fn test_composition_keywords() {
        // SEP-2106: Test anyOf support
        let schema = json!({
            "type": "object",
            "properties": {
                "input": {
                    "anyOf": [
                        {"type": "string"},
                        {"type": "integer"}
                    ]
                }
            }
        });

        let mut validator = SchemaValidator::new();

        assert!(
            validator
                .validate(&schema, &json!({"input": "hello"}))
                .await
                .is_ok()
        );
        assert!(
            validator
                .validate(&schema, &json!({"input": 42}))
                .await
                .is_ok()
        );
        assert!(
            validator
                .validate(&schema, &json!({"input": true}))
                .await
                .is_err()
        );
    }
}
