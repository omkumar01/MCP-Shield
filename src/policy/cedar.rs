//! Amazon Cedar policy evaluation engine.
//!
//! **Phase 2 — PRODUCTION.** Embeds the `cedar-policy` crate for synchronous,
//! sub-millisecond Attribute-Based Access Control (ABAC).
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
pub use cedar_policy::Decision;
use cedar_policy::{
    Authorizer, Context, Entities, Entity, EntityAttrEvaluationError, EntityUid, ParseErrors,
    PolicyId, PolicySet, Request, RequestValidationError, RestrictedExpression,
    RestrictedExpressionParseError, entities_errors::EntitiesError,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::HashSet as StdHashSet;
use std::path::Path;
use tokio::sync::RwLock;

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
    /// The final decision (re-exported from cedar_policy).
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

impl From<ParseErrors> for CedarError {
    fn from(e: ParseErrors) -> Self {
        CedarError::ParseError(format!("{:#}", e))
    }
}

impl From<std::io::Error> for CedarError {
    fn from(e: std::io::Error) -> Self {
        CedarError::IoError(e.to_string())
    }
}

impl From<EntitiesError> for CedarError {
    fn from(e: EntitiesError) -> Self {
        CedarError::EvaluationError(format!("{:#}", e))
    }
}

impl From<RequestValidationError> for CedarError {
    fn from(e: RequestValidationError) -> Self {
        CedarError::EvaluationError(format!("{:#}", e))
    }
}

impl From<RestrictedExpressionParseError> for CedarError {
    fn from(e: RestrictedExpressionParseError) -> Self {
        CedarError::EvaluationError(format!("{:#}", e))
    }
}

impl From<EntityAttrEvaluationError> for CedarError {
    fn from(e: EntityAttrEvaluationError) -> Self {
        CedarError::EvaluationError(format!("{:#}", e))
    }
}

/// A stub Cedar authorizer for Phase 1/testing.
///
/// Always returns `Allow` — real authorization is provided by `CedarPolicyAuthorizer`.
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
            diagnostics: vec!["Stub authorizer: all requests allowed".to_string()],
        })
    }

    async fn reload_policies(&self) -> Result<(), CedarError> {
        tracing::debug!("StubAuthorizer::reload_policies() is a no-op");
        Ok(())
    }

    async fn validate_policy(&self, _policy_text: &str) -> Result<(), CedarError> {
        Ok(())
    }
}

/// Production Cedar policy authorizer using the `cedar-policy` crate.
///
/// Evaluates MCP requests against a loaded policy set with sub-millisecond
/// deterministic ABAC decisions.
///
/// # Entity model
///
/// The authorizer builds Cedar entities per-request:
/// - **Principal**: `Client::"<principal_id>"` with a `scopes` attribute (Cedar `Set<String>`).
/// - **Action**: `Action::"<mcp_method>"` (e.g. `Action::"tools/call"`).
/// - **Resource**: `Tool::"<qualified_name>"` for tool calls, `Resource::"<resource_id>"` for reads.
///
/// Policies in `policies/default.cedar` reference these entity types:
/// ```cedar
/// permit(principal == Client::"authenticated", action == Action::"tools/call",
///        resource == Tool::"com.echo.echo")
/// when { principal has scopes && principal.scopes.contains("mcp:tools:call") };
/// ```
pub struct CedarPolicyAuthorizer {
    /// The policy source text (kept for reloads).
    policy_source: RwLock<String>,

    /// The compiled policy set.
    policy_set: RwLock<PolicySet>,

    /// The cached authorizer.
    authorizer: Authorizer,
}

impl CedarPolicyAuthorizer {
    /// Create a new Cedar policy authorizer with the given policy text.
    ///
    /// Parses the policy text immediately; returns an error if parsing fails.
    pub fn new(policy_text: impl Into<String>) -> Result<Self, CedarError> {
        let source = policy_text.into();
        let policy_set = Self::parse_policies(&source)?;
        let authorizer = Authorizer::new();
        Ok(Self {
            policy_source: RwLock::new(source),
            policy_set: RwLock::new(policy_set),
            authorizer,
        })
    }

    /// Load Cedar policies from a file path.
    ///
    /// Reads the file and parses it as a Cedar policy set.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, CedarError> {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path)?;
        tracing::info!(path = %path.display(), "Loading Cedar policies from file");
        Self::new(source)
    }

    /// Parse a policy string into a `PolicySet`.
    fn parse_policies(source: &str) -> Result<PolicySet, CedarError> {
        let policy_set: PolicySet = source.parse()?;
        Ok(policy_set)
    }

    /// Parse a Cedar entity UID string like `"Client::\"authenticated\""` into
    /// an `EntityUid`.
    fn parse_euid(euid_str: &str) -> Result<EntityUid, CedarError> {
        euid_str.parse::<EntityUid>().map_err(|e| {
            CedarError::EvaluationError(format!("Invalid entity UID '{}': {}", euid_str, e))
        })
    }

    /// Build a `Client` entity with the given scopes.
    ///
    /// The entity carries a `scopes` attribute as a `Cedar Set<String>`, matching
    /// the `when { principal.scopes.contains("...") }` clauses in the policy file.
    fn build_principal_entity(principal_id: &str, scopes: &[String]) -> Result<Entity, CedarError> {
        let euid = Self::parse_euid(&format!(r#"Client::"{}""#, principal_id))?;

        // Build the scopes set as RestrictedExpression: Set literal like ["a", "b"]
        let scope_list: Vec<String> = scopes.iter().map(|s| format!("\"{}\"", s)).collect();
        let scopes_set_expr = format!("[{}]", scope_list.join(","));
        let scopes_set = scopes_set_expr.parse::<RestrictedExpression>()?;

        let attrs = HashMap::from([("scopes".to_string(), scopes_set)]);
        Ok(Entity::new(euid, attrs, StdHashSet::new())?)
    }

    /// Build an `Action` entity (no attributes needed).
    fn build_action_entity(action: &str) -> Result<Entity, CedarError> {
        let euid = Self::parse_euid(&format!(r#"Action::"{}""#, action))?;
        Ok(Entity::new_no_attrs(euid, StdHashSet::new()))
    }

    /// Build a `Resource` entity for tool calls.
    ///
    /// Uses `Tool::"<qualified_name>"` as the entity type for tool operations,
    /// and `Resource::"<id>"` for non-tool resources.
    fn build_resource_entity(resource_id: &str, is_tool: bool) -> Result<Entity, CedarError> {
        let euid = if is_tool {
            Self::parse_euid(&format!(r#"Tool::"{}""#, resource_id))?
        } else {
            Self::parse_euid(&format!(r#"Resource::"{}""#, resource_id))?
        };
        Ok(Entity::new_no_attrs(euid, StdHashSet::new()))
    }
}

#[async_trait]
impl CedarAuthorizer for CedarPolicyAuthorizer {
    async fn evaluate(
        &self,
        request: &AuthorizationRequest,
    ) -> Result<AuthorizationResponse, CedarError> {
        // Extract scopes from context if present; otherwise principal has no scopes.
        let scopes: Vec<String> = request
            .context
            .get("scopes")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        // Build entities
        let principal = Self::build_principal_entity(&request.principal, &scopes)?;
        let principal_euid = principal.uid().clone();
        let action = Self::build_action_entity(&request.action)?;
        let action_euid = action.uid().clone();

        // Determine if this is a tool resource (action is "tools/call")
        let is_tool_call = request.action == "tools/call";
        let resource = Self::build_resource_entity(&request.resource, is_tool_call)?;
        let resource_euid = resource.uid().clone();

        // Build Entities collection (no schema = schema-free evaluation)
        let entities = Entities::from_entities([principal, action, resource], None)?;

        // Build the authorization request (schema-free)
        let req = Request::new(
            principal_euid,
            action_euid,
            resource_euid,
            Context::empty(),
            None,
        )?;

        // Evaluate
        let policy_set = self.policy_set.read().await;
        let response = self.authorizer.is_authorized(&req, &policy_set, &entities);

        let decision = response.decision();

        // Extract deciding policy IDs from diagnostics
        let deciding_policies: Vec<String> = response
            .diagnostics()
            .reason()
            .map(|id| id.to_string())
            .collect();

        let diagnostics: Vec<String> = response
            .diagnostics()
            .reason()
            .map(|id| {
                format!(
                    "policy {} contributed to {}",
                    id,
                    match decision {
                        Decision::Allow => "ALLOW",
                        Decision::Deny => "DENY",
                    }
                )
            })
            .collect();

        Ok(AuthorizationResponse {
            decision,
            deciding_policies,
            diagnostics,
        })
    }

    async fn reload_policies(&self) -> Result<(), CedarError> {
        let source = self.policy_source.read().await;
        let new_set = Self::parse_policies(&source)?;
        let mut set = self.policy_set.write().await;
        *set = new_set;
        tracing::info!("Cedar policies reloaded successfully");
        Ok(())
    }

    async fn validate_policy(&self, policy_text: &str) -> Result<(), CedarError> {
        // Parse-only; don't store
        let _: PolicySet = policy_text
            .parse()
            .map_err(|e: ParseErrors| CedarError::ValidationError(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_POLICY: &str = r#"
permit (
    principal == Client::"authenticated",
    action == Action::"tools/list",
    resource
);

permit (
    principal == Client::"authenticated",
    action == Action::"tools/call",
    resource == Tool::"com.echo.echo"
)
when {
    principal has scopes && principal.scopes.contains("mcp:tools:call")
};

permit (
    principal == Client::"authenticated",
    action == Action::"shutdown",
    resource
)
when {
    principal has scopes && principal.scopes.contains("mcp:admin")
};
"#;

    fn make_authorizer() -> CedarPolicyAuthorizer {
        CedarPolicyAuthorizer::new(TEST_POLICY.to_string()).unwrap()
    }

    #[tokio::test]
    async fn test_permit_tools_list() {
        let authz = make_authorizer();
        let req = AuthorizationRequest {
            principal: "authenticated".into(),
            action: "tools/list".into(),
            resource: "any".into(),
            context: HashMap::new(),
        };
        let resp = authz.evaluate(&req).await.unwrap();
        assert_eq!(resp.decision, Decision::Allow);
    }

    #[tokio::test]
    async fn test_permit_tools_call_with_scope() {
        let authz = make_authorizer();
        let req = AuthorizationRequest {
            principal: "authenticated".into(),
            action: "tools/call".into(),
            resource: "com.echo.echo".into(),
            context: HashMap::from([(
                "scopes".into(),
                serde_json::json!(["mcp:tools:read", "mcp:tools:call"]),
            )]),
        };
        let resp = authz.evaluate(&req).await.unwrap();
        assert_eq!(resp.decision, Decision::Allow);
        assert!(!resp.deciding_policies.is_empty());
    }

    #[tokio::test]
    async fn test_deny_tools_call_without_scope() {
        let authz = make_authorizer();
        let req = AuthorizationRequest {
            principal: "authenticated".into(),
            action: "tools/call".into(),
            resource: "com.echo.echo".into(),
            context: HashMap::new(),
        };
        let resp = authz.evaluate(&req).await.unwrap();
        assert_eq!(resp.decision, Decision::Deny);
    }

    #[tokio::test]
    async fn test_forbid_shutdown_without_admin() {
        let authz = make_authorizer();
        let req = AuthorizationRequest {
            principal: "authenticated".into(),
            action: "shutdown".into(),
            resource: "any".into(),
            context: HashMap::new(),
        };
        let resp = authz.evaluate(&req).await.unwrap();
        assert_eq!(resp.decision, Decision::Deny);
    }

    #[tokio::test]
    async fn test_forbid_shutdown_with_admin_is_allowed() {
        let authz = make_authorizer();
        let req = AuthorizationRequest {
            principal: "authenticated".into(),
            action: "shutdown".into(),
            resource: "any".into(),
            context: HashMap::from([(
                "scopes".into(),
                serde_json::json!(["mcp:tools:read", "mcp:admin"]),
            )]),
        };
        let resp = authz.evaluate(&req).await.unwrap();
        assert_eq!(resp.decision, Decision::Allow);
    }

    #[tokio::test]
    async fn test_validate_good_policy() {
        let authz = make_authorizer();
        assert!(
            authz
                .validate_policy("permit(principal, action, resource);")
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn test_validate_bad_policy() {
        let authz = make_authorizer();
        // Missing semicolon → parse error
        let result = authz
            .validate_policy("permit(principal, action, resource)")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_reload_policies() {
        let authz = make_authorizer();
        assert!(authz.reload_policies().await.is_ok());
    }

    #[tokio::test]
    async fn test_stub_authorizer_always_allows() {
        let authz = StubAuthorizer;
        let req = AuthorizationRequest {
            principal: "anyone".into(),
            action: "shutdown".into(),
            resource: "anything".into(),
            context: HashMap::new(),
        };
        let resp = authz.evaluate(&req).await.unwrap();
        assert_eq!(resp.decision, Decision::Allow);
    }
}
