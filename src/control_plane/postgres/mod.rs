//! PostgreSQL-backed control plane implementation.
//!
//! This module is only compiled when the `postgres` feature is enabled.

#[cfg(feature = "postgres")]
use super::*;
#[cfg(feature = "postgres")]
use sqlx::{PgPool, Row};
#[cfg(feature = "postgres")]
use std::time::Duration;

#[cfg(feature = "postgres")]
/// PostgreSQL control plane implementation.
pub struct PostgresControlPlane {
    pool: PgPool,
    cache: Arc<InMemoryControlPlane>,
}

#[cfg(feature = "postgres")]
impl PostgresControlPlane {
    /// Create a new PostgreSQL control plane.
    ///
    /// # Arguments
    /// * `database_url` - PostgreSQL connection string
    /// * `cache_ttl_secs` - How long to cache data in memory (0 = no cache)
    pub async fn new(database_url: &str, cache_ttl_secs: u64) -> Result<Self, ControlPlaneError> {
        let pool = PgPool::connect(database_url)
            .await
            .map_err(|e| ControlPlaneError::Database(format!("Failed to connect: {}", e)))?;

        // Run migrations if needed
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(|e| ControlPlaneError::Database(format!("Migration failed: {}", e)))?;

        let cache = Arc::new(InMemoryControlPlane::new());

        // If caching enabled, start background refresh
        if cache_ttl_secs > 0 {
            let pool_clone = pool.clone();
            let cache_clone = cache.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(cache_ttl_secs));
                loop {
                    interval.tick().await;
                    if let Err(e) = Self::refresh_cache(&pool_clone, &cache_clone).await {
                        tracing::warn!(error = %e, "Failed to refresh control plane cache");
                    }
                }
            });
        }

        Ok(Self { pool, cache })
    }

    /// Refresh the in-memory cache from database.
    async fn refresh_cache(
        pool: &PgPool,
        cache: &InMemoryControlPlane,
    ) -> Result<(), ControlPlaneError> {
        // Load tenants
        let tenants: Vec<Tenant> = sqlx::query_as!(
            Tenant,
            r#"SELECT tenant_id, name, active, config, created_at, updated_at FROM tenants WHERE active = true"#
        )
        .fetch_all(pool)
        .await
        .map_err(|e| ControlPlaneError::Database(e.to_string()))?;

        // Load policies
        let policies: Vec<PolicyEntry> = sqlx::query_as!(
            PolicyEntry,
            r#"SELECT policy_id, tenant_id, policy_text, enabled, version, created_at, updated_at FROM policies WHERE enabled = true"#
        )
        .fetch_all(pool)
        .await
        .map_err(|e| ControlPlaneError::Database(e.to_string()))?;

        // Load upstreams
        let upstreams: Vec<UpstreamEntry> = sqlx::query_as!(
            UpstreamEntry,
            r#"SELECT server_id, tenant_id, transport, url, active, created_at, updated_at FROM upstream_servers WHERE active = true"#
        )
        .fetch_all(pool)
        .await
        .map_err(|e| ControlPlaneError::Database(e.to_string()))?;

        // Load rate limit rules
        let rules: Vec<RateLimitRule> = sqlx::query_as!(
            RateLimitRule,
            r#"SELECT rule_id, tenant_id, name, scope, scope_value, algorithm, requests_per_window, window_seconds, burst_allowance, action, priority, enabled, created_at, updated_at FROM rate_limit_rules WHERE enabled = true"#
        )
        .fetch_all(pool)
        .await
        .map_err(|e| ControlPlaneError::Database(e.to_string()))?;

        // Update cache
        let new_cache = InMemoryControlPlane::with_config(tenants, policies, upstreams, rules);
        *cache.tenants.write().await = new_cache.tenants.read().await.clone();
        *cache.policies.write().await = new_cache.policies.read().await.clone();
        *cache.upstreams.write().await = new_cache.upstreams.read().await.clone();
        *cache.rate_limit_rules.write().await = new_cache.rate_limit_rules.read().await.clone();

        Ok(())
    }

    /// Force a cache refresh.
    pub async fn refresh(&self) -> Result<(), ControlPlaneError> {
        Self::refresh_cache(&self.pool, &self.cache).await
    }
}

#[cfg(feature = "postgres")]
#[async_trait::async_trait]
impl ControlPlane for PostgresControlPlane {
    async fn load_tenants(&self) -> Result<Vec<Tenant>, ControlPlaneError> {
        self.cache.load_tenants().await
    }

    async fn load_policies(&self, tenant_id: &str) -> Result<Vec<PolicyEntry>, ControlPlaneError> {
        self.cache.load_policies(tenant_id).await
    }

    async fn load_upstreams(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<UpstreamEntry>, ControlPlaneError> {
        self.cache.load_upstreams(tenant_id).await
    }

    async fn load_rate_limit_rules(
        &self,
        tenant_id: &str,
    ) -> Result<Vec<RateLimitRule>, ControlPlaneError> {
        self.cache.load_rate_limit_rules(tenant_id).await
    }

    async fn update_policy(&self, policy: PolicyEntry) -> Result<(), ControlPlaneError> {
        // Update database
        sqlx::query!(
            r#"UPDATE policies SET policy_text = $1, enabled = $2, version = version + 1, updated_at = NOW() WHERE policy_id = $3"#,
            policy.policy_text,
            policy.enabled,
            policy.policy_id
        )
        .execute(&self.pool)
        .await
        .map_err(|e| ControlPlaneError::Database(e.to_string()))?;

        // Update cache
        self.cache.update_policy(policy).await
    }

    async fn create_tenant(&self, tenant: Tenant) -> Result<(), ControlPlaneError> {
        sqlx::query!(
            r#"INSERT INTO tenants (tenant_id, name, active, config, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, $6)"#,
            tenant.tenant_id,
            tenant.name,
            tenant.active,
            tenant.config,
            tenant.created_at,
            tenant.updated_at
        )
        .execute(&self.pool)
        .await
        .map_err(|e| ControlPlaneError::Database(e.to_string()))?;

        self.cache.create_tenant(tenant).await
    }

    async fn get_tenant(&self, tenant_id: &str) -> Result<Option<Tenant>, ControlPlaneError> {
        self.cache.get_tenant(tenant_id).await
    }
}
