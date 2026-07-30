//! Authentication and authorization layer.
//!
//! Provides JWT validation, OAuth 2.1 interception, and fine-grained
//! scope enforcement for MCP-Shield.

pub mod jwt;
pub mod oauth;
pub mod scope;

pub use jwt::{JwtClaims, JwtValidator, JwtValidatorConfig};
pub use oauth::{unauthorized_response, forbidden_response, protected_resource_metadata, authorization_server_metadata};
pub use scope::ScopeEnforcer;
