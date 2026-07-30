//! Amazon Cedar policy evaluation engine.
//!
//! **Phase 2 — STUB.** This module defines the trait contract for Cedar-based
//! authorization. The full implementation will embed the `cedar-policy` crate
//! for synchronous, sub-millisecond Attribute-Based Access Control (ABAC).
//!
//! Cedar provides deterministic, mathematically-verifiable policy evaluation
//! without LLM latency. Policies are written in the Cedar policy language:
//!
//! ```cedar
//! permit(
//!     principal == Client::"service-a",
//!     action == Action::"tools/call",
//!     resource == Tool::"com.example:echo"
//! )
//! when {
//!     principal has scopes && principal.scopes.contains("mcp:tools:call")
//! };
//! ```

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The result of a Cedar policy evaluation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    /// The request is allowed.
    Allow,
    /// The request is denied.
    Deny,
}

/// A Cedar authorization request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizationRequest {
    /// The principal (client) UID, e.g., `Client::"service-a"`.
    pub principal: String,

    /// The action UID, e.g., `Action::"tools/call"`.
    pub action: String,

    /// The resource UID, e.g., `Tool::"com.example:echo"`.
    pub resource: String,

    /// Additional context attributes for policy evaluation.
    #[serde(default)]
    pub context: HashMap<String, serde_json::Value>,
}

/// A Cedar authorization response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizationResponse {
    /// The final decision.
    pub decision: Decision,

    /// The IDs of policies that contributed to the decision.
    pub deciding_policies: Vec<String>,

    /// Diagnostic information.
    pub diagnostics: Vec<String>,
}

/// Trait for Cedar-based authorization.
///
/// Implementations evaluate MCP requests against a policy set to produce
/// deterministic allow/deny decisions in sub-millisecond time.
#[async_trait]
pub trait CedarAuthorizer: Send + Sync {
    /// Evaluate an authorization request against the loaded policy set.
    async fn evaluate(
        &self,
        request: &AuthorizationRequest,
    ) -> Result<AuthorizationResponse, CedarError>;

    /// Reload the policy set from the control plane.
    async fn reload_policies(&self) -> Result<(), CedarError>;

    /// Validate a policy string without loading it.
    async fn validate_policy(&self, policy_text: &str) -> Result<(), CedarError>;
}

/// Error type for Cedar authorization.
#[derive(Debug, thiserror::Error)]
pub enum CedarError {
    #[error("Policy parse error: {0}")]
    ParseError(String),

    #[error("Policy validation error: {0}")]
    ValidationError(String),

    #[error("Evaluation error: {0}")]
    EvaluationError(String),

    #[error("IO error: {0}")]
    IoError(String),
}

/// A stub Cedar authorizer for Phase 1.
///
/// Always returns `Allow` — real authorization is deferred to Phase 2.
pub struct StubAuthorizer;

#[async_trait]
impl CedarAuthorizer for StubAuthorizer {
    async fn evaluate(
        &self,
        _request: &AuthorizationRequest,
    ) -> Result<AuthorizationResponse, CedarError> {
        Ok(AuthorizationResponse {
            decision: Decision::Allow,
            deciding_policies: vec!["stub-permit-all".to_string()],
            diagnostics: vec!["Phase 1 stub: all requests allowed".to_string()],
        })
    }

    async fn reload_policies(&self) -> Result<(), CedarError> {
        tracing::warn!("StubAuthorizer::reload_policies() is a no-op (Phase 2)");
        Ok(())
    }

    async fn validate_policy(&self, _policy_text: &str) -> Result<(), CedarError> {
        Ok(())
    }
}

/// Production Cedar authorizer using the `cedar-policy` crate.
///
/// **TODO (Phase 2):** Implement using `cedar_policy::Authorizer`.
/// The implementation will:
/// 1. Parse policies from text into a `PolicySet`
/// 2. Build an `Entities` collection from the control plane
/// 3. Construct a `Request` from the MCP method, principal, and resource
/// 4. Call `Authorizer::is_authorized()` for synchronous evaluation
/// 5. Cache compiled policy sets for low latency
pub struct CedarPolicyAuthorizer {
    policy_text: String,
}

impl CedarPolicyAuthorizer {
    /// Create a new Cedar policy authorizer with the given policy text.
    pub fn new(policy_text: impl Into<String>) -> Self {
        Self {
            policy_text: policy_text.into(),
        }
    }
}

#[async_trait]
impl CedarAuthorizer for CedarPolicyAuthorizer {
    async fn evaluate(
        &self,
        _request: &AuthorizationRequest,
    ) -> Result<AuthorizationResponse, CedarError> {
        // TODO (Phase 2): Implement using cedar_policy::Authorizer
        tracing::warn!(
            policy_len = self.policy_text.len(),
            "CedarPolicyAuthorizer::evaluate() is a stub (Phase 2)"
        );
        Ok(AuthorizationResponse {
            decision: Decision::Allow,
            deciding_policies: vec![],
            diagnostics: vec!["Phase 2 stub: real Cedar evaluation pending".to_string()],
        })
    }

    async fn reload_policies(&self) -> Result<(), CedarError> {
        tracing::warn!("CedarPolicyAuthorizer::reload_policies() is a stub (Phase 2)");
        Ok(())
    }

    async fn validate_policy(&self, policy_text: &str) -> Result<(), CedarError> {
        // TODO (Phase 2): Use cedar_policy::PolicySet::from_str
        tracing::debug!(policy_len = policy_text.len(), "Policy validation stub");
        Ok(())
    }
}
