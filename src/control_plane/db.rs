//! PostgreSQL-backed control plane.
//!
//! **Phase 4 — STUB.** This module defines the trait contract for the
//! distributed control plane that manages tenant and policy configuration.
//! The full implementation will use `sqlx` for async PostgreSQL access.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
}

/// An upstream server configuration entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamEntry {
    pub server_id: String,
    pub tenant_id: String,
    pub transport: String,
    pub url: Option<String>,
    pub active: bool,
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

    /// Update a policy.
    async fn update_policy(&self, policy: PolicyEntry) -> Result<(), ControlPlaneError>;

    /// Create a new tenant.
    async fn create_tenant(&self, tenant: Tenant) -> Result<(), ControlPlaneError>;
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
}

/// A stub control plane for Phase 1.
pub struct StubControlPlane;

#[async_trait]
impl ControlPlane for StubControlPlane {
    async fn load_tenants(&self) -> Result<Vec<Tenant>, ControlPlaneError> {
        Ok(vec![Tenant {
            tenant_id: "default".to_string(),
            name: "Default Tenant".to_string(),
            active: true,
            config: serde_json::json!({}),
        }])
    }

    async fn load_policies(&self, _tenant_id: &str) -> Result<Vec<PolicyEntry>, ControlPlaneError> {
        Ok(Vec::new())
    }

    async fn load_upstreams(
        &self,
        _tenant_id: &str,
    ) -> Result<Vec<UpstreamEntry>, ControlPlaneError> {
        Ok(Vec::new())
    }

    async fn update_policy(&self, _policy: PolicyEntry) -> Result<(), ControlPlaneError> {
        Ok(())
    }

    async fn create_tenant(&self, _tenant: Tenant) -> Result<(), ControlPlaneError> {
        Ok(())
    }
}

/// PostgreSQL-backed control plane.
///
/// **TODO (Phase 4):** Implement using `sqlx::PgPool`.
pub struct PostgresControlPlane {
    database_url: String,
    cache: tokio::sync::RwLock<HashMap<String, Tenant>>,
}

impl PostgresControlPlane {
    pub fn new(database_url: impl Into<String>) -> Self {
        Self {
            database_url: database_url.into(),
            cache: tokio::sync::RwLock::new(HashMap::new()),
        }
    }

    /// Connect to the database.
    ///
    /// **TODO (Phase 4):** Implement using `sqlx::PgPoolOptions`.
    pub async fn connect(&self) -> Result<(), ControlPlaneError> {
        tracing::warn!(
            database_url = %self.database_url,
            "PostgresControlPlane::connect() is a stub (Phase 4)"
        );
        Ok(())
    }
}

#[async_trait]
impl ControlPlane for PostgresControlPlane {
    async fn load_tenants(&self) -> Result<Vec<Tenant>, ControlPlaneError> {
        let cache = self.cache.read().await;
        Ok(cache.values().cloned().collect())
    }

    async fn load_policies(&self, _tenant_id: &str) -> Result<Vec<PolicyEntry>, ControlPlaneError> {
        tracing::warn!("PostgresControlPlane::load_policies() is a stub (Phase 4)");
        Ok(Vec::new())
    }

    async fn load_upstreams(
        &self,
        _tenant_id: &str,
    ) -> Result<Vec<UpstreamEntry>, ControlPlaneError> {
        tracing::warn!("PostgresControlPlane::load_upstreams() is a stub (Phase 4)");
        Ok(Vec::new())
    }

    async fn update_policy(&self, _policy: PolicyEntry) -> Result<(), ControlPlaneError> {
        tracing::warn!("PostgresControlPlane::update_policy() is a stub (Phase 4)");
        Ok(())
    }

    async fn create_tenant(&self, _tenant: Tenant) -> Result<(), ControlPlaneError> {
        tracing::warn!("PostgresControlPlane::create_tenant() is a stub (Phase 4)");
        Ok(())
    }
}
