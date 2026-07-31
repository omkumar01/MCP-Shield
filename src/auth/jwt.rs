//! JWT validation for MCP-Shield.
//!
//! Validates Bearer tokens from the `Authorization` header using either
//! a shared HMAC secret or RSA/EC keys from a JWKS endpoint.

use crate::error::McpError;
use jsonwebtoken::{Algorithm, DecodingKey, Header, TokenData, Validation, decode, decode_header};
use serde::{Deserialize, Serialize};
#[cfg(test)]
use serde_json::{Value, json};
#[cfg(not(test))]
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Claims extracted from a validated JWT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    /// Subject (typically the client ID).
    pub sub: Option<String>,

    /// Issuer.
    pub iss: Option<String>,

    /// Audience(s).
    pub aud: Option<HashSet<String>>,

    /// Expiration time (Unix timestamp).
    pub exp: Option<u64>,

    /// Not-before time (Unix timestamp).
    pub nbf: Option<u64>,

    /// Issued-at time (Unix timestamp).
    pub iat: Option<u64>,

    /// OAuth 2.1 scopes (space-delimited in the JWT, parsed into a Vec).
    pub scope: Option<Vec<String>>,

    /// Raw token identifier.
    pub jti: Option<String>,

    /// All additional claims.
    #[serde(flatten)]
    pub extra: Value,
}

/// JWT validator with support for HMAC secrets and JWKS key rotation.
#[derive(Clone)]
pub struct JwtValidator {
    /// Configuration for the validator.
    config: JwtValidatorConfig,

    /// Cached JWKS keys (for RSA/EC validation).
    jwks_keys: Arc<RwLock<Vec<DecodingKey>>>,
}

/// Configuration for the JWT validator.
#[derive(Debug, Clone)]
pub struct JwtValidatorConfig {
    /// HMAC secret (for HS256/HS384/HS512).
    pub hmac_secret: Option<String>,

    /// JWKS URL for key rotation (for RS256/ES256/etc.).
    pub jwks_url: Option<String>,

    /// Expected issuer.
    pub issuer: Option<String>,

    /// Expected audience.
    pub audience: Option<String>,

    /// Allowed algorithms.
    pub algorithms: Vec<Algorithm>,

    /// Whether to validate the expiration claim.
    pub validate_exp: bool,

    /// Leeway for time-based claims (seconds).
    pub leeway: u64,
}

impl Default for JwtValidatorConfig {
    fn default() -> Self {
        Self {
            hmac_secret: None,
            jwks_url: None,
            issuer: None,
            audience: None,
            algorithms: vec![
                Algorithm::RS256,
                Algorithm::RS384,
                Algorithm::RS512,
                Algorithm::ES256,
                Algorithm::ES384,
                Algorithm::HS256,
                Algorithm::HS384,
                Algorithm::HS512,
            ],
            validate_exp: true,
            leeway: 30,
        }
    }
}

impl JwtValidator {
    /// Create a new JWT validator with the given configuration.
    pub fn new(config: JwtValidatorConfig) -> Self {
        Self {
            config,
            jwks_keys: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create a validator using HMAC secret.
    pub fn with_hmac_secret(secret: impl Into<String>, issuer: Option<String>) -> Self {
        let mut config = JwtValidatorConfig::default();
        config.hmac_secret = Some(secret.into());
        config.issuer = issuer;
        config.algorithms = vec![Algorithm::HS256];
        Self::new(config)
    }

    /// Validate a JWT token string and extract claims.
    pub fn validate_token(&self, token: &str) -> Result<JwtClaims, McpError> {
        let header = decode_header(token)
            .map_err(|e| McpError::JwtError(format!("Failed to decode JWT header: {}", e)))?;

        // Determine the decoding key based on algorithm
        let decoding_key = self.get_decoding_key(&header)?;

        // Build validation rules
        let mut validation = Validation::new(header.alg);
        if let Some(ref issuer) = self.config.issuer {
            validation.set_issuer(&[issuer.as_str()]);
        }
        if let Some(ref audience) = self.config.audience {
            validation.set_audience(&[audience.as_str()]);
        }
        validation.validate_exp = self.config.validate_exp;
        validation.leeway = self.config.leeway as u64;
        validation.required_spec_claims = HashSet::new(); // Don't require any specific claims

        let token_data: TokenData<Value> = decode::<Value>(token, &decoding_key, &validation)
            .map_err(|e| McpError::JwtError(format!("JWT validation failed: {}", e)))?;

        // Extract typed claims from the raw value
        self.extract_claims(&header, &token_data)
    }

    /// Get the appropriate decoding key for the given header algorithm.
    fn get_decoding_key(&self, header: &Header) -> Result<DecodingKey, McpError> {
        match header.alg {
            Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512 => {
                let secret = self.config.hmac_secret.as_ref().ok_or_else(|| {
                    McpError::JwtError(
                        "HMAC algorithm requires a configured jwt_secret".to_string(),
                    )
                })?;
                Ok(DecodingKey::from_secret(secret.as_bytes()))
            }
            Algorithm::RS256
            | Algorithm::RS384
            | Algorithm::RS512
            | Algorithm::ES256
            | Algorithm::ES384 => {
                let keys = self.jwks_keys.blocking_read();
                if keys.is_empty() {
                    return Err(McpError::JwtError(format!(
                        "No JWKS keys loaded for algorithm {:?}. Configure jwks_url.",
                        header.alg
                    )));
                }
                // Use the first available key (simplified; production should match kid)
                Ok(keys[0].clone())
            }
            _ => Err(McpError::JwtError(format!(
                "Unsupported JWT algorithm: {:?}",
                header.alg
            ))),
        }
    }

    /// Extract typed claims from the raw token data.
    fn extract_claims(
        &self,
        _header: &Header,
        token_data: &TokenData<Value>,
    ) -> Result<JwtClaims, McpError> {
        let claims = &token_data.claims;

        Ok(JwtClaims {
            sub: claims
                .get("sub")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            iss: claims
                .get("iss")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            aud: claims.get("aud").and_then(|v| {
                if v.is_string() {
                    Some(HashSet::from([v.as_str().unwrap().to_string()]))
                } else if v.is_array() {
                    Some(
                        v.as_array()
                            .unwrap()
                            .iter()
                            .filter_map(|item| item.as_str().map(|s| s.to_string()))
                            .collect(),
                    )
                } else {
                    None
                }
            }),
            exp: claims.get("exp").and_then(|v| v.as_u64()),
            nbf: claims.get("nbf").and_then(|v| v.as_u64()),
            iat: claims.get("iat").and_then(|v| v.as_u64()),
            scope: claims.get("scope").and_then(|v| {
                v.as_str()
                    .map(|s| s.split_whitespace().map(String::from).collect())
            }),
            jti: claims
                .get("jti")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            extra: claims.clone(),
        })
    }

    /// Extract a Bearer token from an Authorization header value.
    pub fn extract_bearer_token(auth_header: &str) -> Result<String, McpError> {
        let parts: Vec<&str> = auth_header.splitn(2, ' ').collect();
        if parts.len() != 2 || parts[0] != "Bearer" {
            return Err(McpError::Unauthorized(
                "Invalid Authorization header format. Expected: Bearer <token>".to_string(),
            ));
        }
        let token = parts[1].trim();
        if token.is_empty() {
            return Err(McpError::Unauthorized("Empty Bearer token".to_string()));
        }
        Ok(token.to_string())
    }

    /// Load JWKS keys from the configured URL.
    ///
    /// This is a placeholder for Phase 2 full implementation.
    pub async fn refresh_jwks(&self) -> Result<(), McpError> {
        if let Some(ref url) = self.config.jwks_url {
            tracing::info!(jwks_url = %url, "Refreshing JWKS keys");
            // TODO: Full JWKS fetch and key parsing in Phase 2
            tracing::warn!("JWKS refresh not yet implemented — configure jwt_secret for HMAC");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{EncodingKey, Header, encode};

    fn create_test_token(secret: &str, claims: Value) -> String {
        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap()
    }

    #[test]
    fn test_extract_bearer_token() {
        assert_eq!(
            JwtValidator::extract_bearer_token("Bearer abc123").unwrap(),
            "abc123"
        );
        assert_eq!(
            JwtValidator::extract_bearer_token("Bearer  token-with-spaces  ").unwrap(),
            "token-with-spaces"
        );
    }

    #[test]
    fn test_reject_invalid_bearer_format() {
        assert!(JwtValidator::extract_bearer_token("Basic abc").is_err());
        assert!(JwtValidator::extract_bearer_token("Bearer").is_err());
        assert!(JwtValidator::extract_bearer_token("").is_err());
    }

    #[test]
    fn test_validate_hmac_token() {
        let validator =
            JwtValidator::with_hmac_secret("test-secret-key-12345", Some("mcp-shield".into()));

        let claims = json!({
            "sub": "client-123",
            "iss": "mcp-shield",
            "scope": "mcp:tools:read mcp:tools:call",
            "exp": (chrono::Utc::now().timestamp() + 3600) as u64
        });

        let token = create_test_token("test-secret-key-12345", claims);
        let result = validator.validate_token(&token);
        assert!(result.is_ok());
        let jwt_claims = result.unwrap();
        assert_eq!(jwt_claims.sub, Some("client-123".into()));
        assert_eq!(jwt_claims.iss, Some("mcp-shield".into()));
        assert!(jwt_claims.scope.is_some());
        let scopes = jwt_claims.scope.unwrap();
        assert!(scopes.contains(&"mcp:tools:read".to_string()));
    }

    #[test]
    fn test_reject_wrong_secret() {
        let validator = JwtValidator::with_hmac_secret("correct-secret", Some("mcp-shield".into()));

        let claims = json!({
            "sub": "client-123",
            "exp": (chrono::Utc::now().timestamp() + 3600) as u64
        });

        let token = create_test_token("wrong-secret", claims);
        assert!(validator.validate_token(&token).is_err());
    }

    #[test]
    fn test_reject_expired_token() {
        let validator = JwtValidator::with_hmac_secret("test-secret", Some("mcp-shield".into()));

        let claims = json!({
            "sub": "client-123",
            "exp": (chrono::Utc::now().timestamp() - 3600) as u64
        });

        let token = create_test_token("test-secret", claims);
        assert!(validator.validate_token(&token).is_err());
    }

    #[test]
    fn test_scope_parsing() {
        let validator = JwtValidator::with_hmac_secret("test-secret", None);

        let claims = json!({
            "sub": "client-123",
            "scope": "mcp:tools:read mcp:tools:call mcp:resources:read",
            "exp": (chrono::Utc::now().timestamp() + 3600) as u64
        });

        let token = create_test_token("test-secret", claims);
        let jwt_claims = validator.validate_token(&token).unwrap();
        let scopes = jwt_claims.scope.unwrap();
        assert_eq!(scopes.len(), 3);
        assert!(scopes.contains(&"mcp:tools:call".to_string()));
    }
}
