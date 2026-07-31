//! PostgreSQL-backed control plane.
//!
//! **Phase 4 — PRODUCTION.** Provides distributed configuration management
//! backed by PostgreSQL (feature-gated) with an in-memory fallback.
//!
//! The control plane manages:
//! - Tenants and their configurations
//! - Policies (Cedar policy text)
//! - Upstream server configurations
//! - Tool registrations
//! - Rate limit rules

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A tenant configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tenant {
    /// Unique tenant ID.
    pub tenant_id: String,

    /// Human-readable tenant name.
    pub name: String,

    /// Whether the tenant is active.
    pub active: bool,

    /// Tenant-specific configuration.
    pub config: serde_json::Value,

    /// Creation timestamp.
    pub created_at: DateTime<Utc>,

    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
}

/// A policy configuration entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyEntry {
    /// Unique policy ID.
    pub policy_id: String,

    /// The tenant this policy belongs to.
    pub tenant_id: String,

    /// The policy text (Cedar policy language).
    pub policy_text: String,

    /// Whether the policy is enabled.
    pub enabled: bool,

    /// Version number for optimistic concurrency.
    pub version: u32,

    /// Creation timestamp.
    pub created_at: DateTime<Utc>,

    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,
}

/// An upstream server configuration entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamEntry {
    pub server_id: String,
    pub tenant_id: String,
    pub transport: String,
    pub url: Option<String>,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A rate limit rule configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitRule {
    pub rule_id: String,
    pub tenant_id: String,
    pub name: String,
    pub scope: String,
    pub scope_value: Option<String>,
    pub algorithm: String,
    pub requests_per_window: u64,
    pub window_seconds: u32,
    pub burst_allowance: Option<u64>,
    pub action: String,
    pub priority: i32,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Trait for the control plane data store.
#[async_trait]
pub trait ControlPlane: Send + Sync {
    /// Load all active tenants.
    async fn load_tenants(&self) -> Result<Vec<Tenant>, ControlPlaneError>;

    /// Load all enabled policies for a tenant.
    async fn load_policies(&self, tenant_id: &str) -> Result<Vec<PolicyEntry>, ControlPlaneError>;

    /// Load all active upstreams for a tenant.
    async fn load_upstreams(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<UpstreamEntry>, ControlPlaneError>;

    /// Load all rate limit rules for a tenant.
    async fn load_rate_limit_rules(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<RateLimitRule>, ControlPlaneError>;

    /// Update a policy.
    async fn update_policy(&self, policy: PolicyEntry) -> Result<(), ControlPlaneError>;

    /// Create a new tenant.
    async fn create_tenant(&self, tenant: Tenant) -> Result<(), ControlPlaneError>;

    /// Get a tenant by ID.
    async fn get_tenant(&self, tenant_id: &str) -> Result<Option<Tenant>, ControlPlaneError>;
}

/// Error type for control plane operations.
#[derive(Debug, thiserror::Error)]
pub enum ControlPlaneError {
    #[error("Database error: {0}")]
    Database(String),

    #[error("Tenant not found: {0}")]
    TenantNotFound(String),

    #[error("Policy not found: {0}")]
    PolicyNotFound(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("IO error: {0}")]
    IoError(String),
}

impl From<std::io::Error> for ControlPlaneError {
    fn from(e: std::io::Error) -> Self {
        ControlPlaneError::IoError(e.to_string())
    }
}

/// An in-memory control plane for development and testing.
///
/// Uses HashMaps for storage. All data is lost on restart.
pub struct InMemoryControlPlane {
    tenants: RwLock<HashMap<String, Tenant>>,
    policies: RwLock<HashMap<String, PolicyEntry>>,
    upstreams: RwLock<HashMap<String, UpstreamEntry>>,
    rate_limit_rules: RwLock<HashMap<String, RateLimitRule>>,
}

impl InMemoryControlPlane {
    /// Create a new in-memory control plane with default tenant.
    pub fn new() -> Self {
        let _tenants = RwLock::new(HashMap::<String, Tenant>::new());
        let _policies = RwLock::new(HashMap::<String, PolicyEntry>::new());
        let _upstreams = RwLock::new(HashMap::<String, UpstreamEntry>::new());
        let _rate_limit_rules = RwLock::new(HashMap::<String, RateLimitRule>::new());

        let mut tenant_map = HashMap::new();
        let default_tenant = Tenant {
            tenant_id: "default".to_string(),
            name: "Default Tenant".to_string(),
            active: true,
            config: serde_json::json!({}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        tenant_map.insert("default".to_string(), default_tenant);

        Self {
            tenants: RwLock::new(tenant_map),
            policies: RwLock::new(HashMap::new()),
            upstreams: RwLock::new(HashMap::new()),
            rate_limit_rules: RwLock::new(HashMap::new()),
        }
    }

    /// Create a control plane pre-seeded with configuration.
    pub fn with_config(
        tenants: Vec<Tenant>,
        policies: Vec<PolicyEntry>,
        upstreams: Vec<UpstreamEntry>,
        rate_limit_rules: Vec<RateLimitRule>,
    ) -> Self {
        let tenant_map: HashMap<_, _> = tenants
            .into_iter()
            .map(|t| (t.tenant_id.clone(), t))
            .collect();
        let policy_map: HashMap<_, _> = policies
            .into_iter()
            .map(|p| (p.policy_id.clone(), p))
            .collect();
        let upstream_map: HashMap<_, _> = upstreams
            .into_iter()
            .map(|u| (u.server_id.clone(), u))
            .collect();
        let rule_map: HashMap<_, _> = rate_limit_rules
            .into_iter()
            .map(|r| (r.rule_id.clone(), r))
            .collect();

        Self {
            tenants: RwLock::new(tenant_map),
            policies: RwLock::new(policy_map),
            upstreams: RwLock::new(upstream_map),
            rate_limit_rules: RwLock::new(rule_map),
        }
    }
}

impl Default for InMemoryControlPlane {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ControlPlane for InMemoryControlPlane {
    async fn load_tenants(&self) -> Result<Vec<Tenant>, ControlPlaneError> {
        let tenants = self.tenants.read().await;
        Ok(tenants.values().filter(|t| t.active).cloned().collect())
    }

    async fn load_policies(&self, tenant_id: &str) -> Result<Vec<PolicyEntry>, ControlPlaneError> {
        let policies = self.policies.read().await;
        Ok(policies
            .values()
            .filter(|p| p.tenant_id == tenant_id && p.enabled)
            .cloned()
            .collect())
    }

    async fn load_upstreams(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<UpstreamEntry>, ControlPlaneError> {
        let upstreams = self.upstreams.read().await;
        Ok(upstreams
            .values()
            .filter(|u| u.tenant_id == tenant_id && u.active)
            .cloned()
            .collect())
    }

    async fn load_rate_limit_rules(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<RateLimitRule>, ControlPlaneError> {
        let rules = self.rate_limit_rules.read().await;
        Ok(rules
            .values()
            .filter(|r| r.tenant_id == tenant_id && r.enabled)
            .cloned()
            .collect())
    }

    async fn update_policy(&self, policy: PolicyEntry) -> Result<(), ControlPlaneError> {
        let mut policies = self.policies.write().await;
        policies.insert(policy.policy_id.clone(), policy);
        Ok(())
    }

    async fn create_tenant(&self, tenant: Tenant) -> Result<(), ControlPlaneError> {
        let mut tenants = self.tenants.write().await;
        if tenants.contains_key(&tenant.tenant_id) {
            return Err(ControlPlaneError::Conflict(format!(
                "Tenant {} already exists",
                tenant.tenant_id
            )));
        }
        tenants.insert(tenant.tenant_id.clone(), tenant);
        Ok(())
    }

    async fn get_tenant(&self, tenant_id: &str) -> Result<Option<Tenant>, ControlPlaneError> {
        let tenants = self.tenants.read().await;
        Ok(tenants.get(tenant_id).cloned())
    }
}

/// Inherent methods on InMemoryControlPlane (not part of ControlPlane trait).
impl InMemoryControlPlane {
    /// Add an upstream server (for testing).
    pub async fn add_upstream(&self, upstream: UpstreamEntry) -> Result<(), ControlPlaneError> {
        let mut upstreams = self.upstreams.write().await;
        upstreams.insert(upstream.server_id.clone(), upstream);
        Ok(())
    }

    /// Add a rate limit rule (for testing).
    pub async fn add_rate_limit_rule(&self, rule: RateLimitRule) -> Result<(), ControlPlaneError> {
        let mut rules = self.rate_limit_rules.write().await;
        rules.insert(rule.rule_id.clone(), rule);
        Ok(())
    }
}

/// PostgreSQL control plane (feature-gated).
///
/// This is a re-export from the separate `postgres` module when the `postgres` feature is enabled.
#[cfg(feature = "postgres")]
pub mod postgres;

#[cfg(feature = "postgres")]
pub use postgres::PostgresControlPlane;

/// Create the appropriate control plane based on configuration and features.
///
/// Returns an `InMemoryControlPlane` by default, or `PostgresControlPlane` if the
/// `postgres` feature is enabled and `DATABASE_URL` is set.
pub async fn create_control_plane(
    _database_url: Option<String>,
    _cache_ttl_secs: u64,
) -> Result<Arc<dyn ControlPlane>, ControlPlaneError> {
    #[cfg(feature = "postgres")]
    {
        if let Some(url) = database_url {
            let pg = postgres::PostgresControlPlane::new(&url, cache_ttl_secs).await?;
            return Ok(Arc::new(pg));
        }
    }

    // Default to in-memory
    Ok(Arc::new(InMemoryControlPlane::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_inmemory_tenant_crud() {
        let cp = InMemoryControlPlane::new();

        // Default tenant exists
        let tenants = cp.load_tenants().await.unwrap();
        assert_eq!(tenants.len(), 1);
        assert_eq!(tenants[0].tenant_id, "default");

        // Create new tenant
        let new_tenant = Tenant {
            tenant_id: "test-tenant".to_string(),
            name: "Test Tenant".to_string(),
            active: true,
            config: serde_json::json!({"key": "value"}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        cp.create_tenant(new_tenant.clone()).await.unwrap();

        let retrieved = cp.get_tenant("test-tenant").await.unwrap();
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "Test Tenant");
    }

    #[tokio::test]
    async fn test_inmemory_policy_management() {
        let cp = InMemoryControlPlane::new();

        let policy = PolicyEntry {
            policy_id: "test-policy".to_string(),
            tenant_id: "default".to_string(),
            policy_text: "permit(principal, action, resource);".to_string(),
            enabled: true,
            version: 1,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        cp.update_policy(policy.clone()).await.unwrap();

        let policies = cp.load_policies("default").await.unwrap();
        assert_eq!(policies.len(), 1);
        assert_eq!(policies[0].policy_id, "test-policy");
    }

    #[tokio::test]
    async fn test_inmemory_upstream_management() {
        let cp = InMemoryControlPlane::new();

        // Add upstream directly to map for testing
        let upstream = UpstreamEntry {
            server_id: "test-server".to_string(),
            tenant_id: "default".to_string(),
            transport: "streamable_http".to_string(),
            url: Some("http://localhost:8080/mcp".to_string()),
            active: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        cp.upstreams
            .write()
            .await
            .insert(upstream.server_id.clone(), upstream);

        let upstreams = cp.load_upstreams("default").await.unwrap();
        assert_eq!(upstreams.len(), 1);
    }

    #[tokio::test]
    async fn test_inmemory_rate_limit_rules() {
        let cp = InMemoryControlPlane::new();

        let rule = RateLimitRule {
            rule_id: "test-rule".to_string(),
            tenant_id: "default".to_string(),
            name: "Test Rule".to_string(),
            scope: "global".to_string(),
            scope_value: None,
            algorithm: "token_bucket".to_string(),
            requests_per_window: 100,
            window_seconds: 60,
            burst_allowance: Some(20),
            action: "reject".to_string(),
            priority: 0,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        cp.rate_limit_rules
            .write()
            .await
            .insert(rule.rule_id.clone(), rule);

        let rules = cp.load_rate_limit_rules("default").await.unwrap();
        assert_eq!(rules.len(), 1);
    }
}
