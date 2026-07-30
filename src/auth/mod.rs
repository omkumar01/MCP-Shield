//! Authentication and authorization layer.
//!
//! Provides JWT validation, OAuth 2.1 interception, and fine-grained
//! scope enforcement for MCP-Shield.

pub mod jwt;
pub mod oauth;
pub mod scope;

pub use jwt::{JwtClaims, JwtValidator, JwtValidatorConfig};
pub use oauth::{
    authorization_server_metadata, forbidden_response, protected_resource_metadata,
    unauthorized_response,
};
pub use scope::ScopeEnforcer;
