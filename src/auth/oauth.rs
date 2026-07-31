//! OAuth 2.1 interceptor for MCP-Shield.
//!
//! Implements the MCP authorization flow:
//! - Returns 401 Unauthorized with WWW-Authenticate header for unauthenticated requests
//! - Serves Protected Resource Metadata (PRM) document
//! - Serves OAuth Authorization Server metadata (OIDC discovery)

use axum::{
    Json, Router,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::json;

/// Build a 401 Unauthorized response with the WWW-Authenticate header.
///
/// Per the MCP authorization specification, the WWW-Authenticate header
/// must point to the Protected Resource Metadata (PRM) document.
pub fn unauthorized_response(prm_url: &str) -> Response {
    let mut response = (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "jsonrpc": "2.0",
            "error": {
                "code": -32001,
                "message": "Unauthorized: valid OAuth 2.1 Bearer token required"
            },
            "id": null
        })),
    )
        .into_response();

    // Add the WWW-Authenticate header pointing to the PRM
    response.headers_mut().insert(
        axum::http::header::WWW_AUTHENTICATE,
        format!("Bearer resource_metadata=\"{}\"", prm_url)
            .parse()
            .unwrap(),
    );

    response
}

/// Build a 403 Forbidden response for scope denial.
pub fn forbidden_response(message: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "jsonrpc": "2.0",
            "error": {
                "code": -32002,
                "message": message
            },
            "id": null
        })),
    )
        .into_response()
}

/// Generate the Protected Resource Metadata (PRM) document.
///
/// The PRM document tells the client where to find the authorization server.
/// Served at `/.well-known/oauth-protected-resource`.
pub fn protected_resource_metadata(
    resource_url: &str,
    authorization_servers: &[String],
) -> serde_json::Value {
    json!({
        "resource": resource_url,
        "authorization_servers": authorization_servers,
    })
}

/// Generate the OAuth Authorization Server metadata document.
///
/// This is the OIDC discovery document that describes the authorization
/// server's endpoints and capabilities.
/// Served at `/.well-known/oauth-authorization-server`.
pub fn authorization_server_metadata(
    issuer: &str,
    authorization_endpoint: &str,
    token_endpoint: &str,
    jwks_uri: &str,
    scopes_supported: &[String],
) -> serde_json::Value {
    json!({
        "issuer": issuer,
        "authorization_endpoint": authorization_endpoint,
        "token_endpoint": token_endpoint,
        "jwks_uri": jwks_uri,
        "scopes_supported": scopes_supported,
        "code_challenge_methods_supported": ["S256"],
        "grant_types_supported": [
            "authorization_code",
            "refresh_token",
            "client_credentials"
        ],
        "response_types_supported": ["code"],
        "token_endpoint_auth_methods_supported": [
            "client_secret_basic",
            "client_secret_post",
            "private_key_jwt"
        ],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": [
            "RS256",
            "RS384",
            "RS512",
            "ES256",
            "ES384"
        ]
    })
}

/// Axum handler for the PRM endpoint.
///
/// Returns the PRM JSON document at `/.well-known/oauth-protected-resource`.
async fn prm_handler(
    resource_url: String,
    authorization_servers: Vec<String>,
) -> impl IntoResponse {
    let prm = protected_resource_metadata(&resource_url, &authorization_servers);
    (StatusCode::OK, Json(prm))
}

/// Axum handler for the authorization server metadata endpoint.
///
/// Returns the OIDC discovery document at `/.well-known/oauth-authorization-server`.
async fn as_metadata_handler(
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
    scopes_supported: Vec<String>,
) -> impl IntoResponse {
    let metadata = authorization_server_metadata(
        &issuer,
        &authorization_endpoint,
        &token_endpoint,
        &jwks_uri,
        &scopes_supported,
    );
    (StatusCode::OK, Json(metadata))
}

/// Create OAuth discovery routes.
///
/// This can be mounted in the main router to expose the OAuth endpoints.
/// The handlers extract state from the request extensions (set by main.rs).
pub fn oauth_discovery_routes(
    resource_url: String,
    authorization_server_url: String,
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
    scopes_supported: Vec<String>,
) -> Router {
    Router::new()
        .route(
            "/.well-known/oauth-protected-resource",
            get(move || {
                let resource_url = resource_url.clone();
                let authorization_servers = vec![authorization_server_url.clone()];
                async move {
                    let prm = protected_resource_metadata(&resource_url, &authorization_servers);
                    (StatusCode::OK, Json(prm))
                }
            }),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(move || {
                let issuer = issuer.clone();
                let authorization_endpoint = authorization_endpoint.clone();
                let token_endpoint = token_endpoint.clone();
                let jwks_uri = jwks_uri.clone();
                let scopes_supported = scopes_supported.clone();
                async move {
                    let metadata = authorization_server_metadata(
                        &issuer,
                        &authorization_endpoint,
                        &token_endpoint,
                        &jwks_uri,
                        &scopes_supported,
                    );
                    (StatusCode::OK, Json(metadata))
                }
            }),
        )
}
