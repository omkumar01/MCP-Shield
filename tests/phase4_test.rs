//! Phase 4 integration tests — Control plane and rate limiter.

use chrono::Utc;
use mcp_shield::{
    control_plane::{
        ControlPlane, InMemoryControlPlane, PolicyEntry, RateLimitRule, Tenant, UpstreamEntry,
    },
    telemetry::rate_limiter::{
        RateLimitAction, RateLimitAlgorithm, RateLimitKey, RateLimitRule as TelemetryRateLimitRule,
        RateLimiter as RateLimiterTrait, TokenBucketRateLimiter,
    },
};
use std::sync::Arc;

// Re-export the internal types we need for the test
use mcp_shield::telemetry::rate_limiter::RateLimitAction as InternalRateLimitAction;
use mcp_shield::telemetry::rate_limiter::RateLimitAlgorithm as InternalRateLimitAlgorithm;
use mcp_shield::telemetry::rate_limiter::RateLimitKey as InternalRateLimitKey;
use mcp_shield::telemetry::rate_limiter::RateLimitRule as InternalRateLimitRule;
use mcp_shield::telemetry::rate_limiter::TokenBucketRateLimiter as InternalTokenBucketRateLimiter;

#[tokio::test]
async fn test_control_plane_tenant_management() {
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
        config: serde_json::json!({"region": "us-east"}),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    cp.create_tenant(new_tenant.clone()).await.unwrap();

    let retrieved = cp.get_tenant("test-tenant").await.unwrap();
    assert!(retrieved.is_some());
    assert_eq!(retrieved.as_ref().unwrap().name, "Test Tenant");
    assert_eq!(retrieved.as_ref().unwrap().config["region"], "us-east");
}

#[tokio::test]
async fn test_control_plane_policy_management() {
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
    assert!(policies[0].enabled);

    // Update policy
    let updated_policy = PolicyEntry {
        version: 2,
        ..policy
    };
    cp.update_policy(updated_policy).await.unwrap();

    let policies = cp.load_policies("default").await.unwrap();
    assert_eq!(policies[0].version, 2);
}

#[tokio::test]
async fn test_control_plane_upstream_management() {
    let cp = InMemoryControlPlane::new();

    // Add upstream using public method
    let upstream = UpstreamEntry {
        server_id: "test-server".to_string(),
        tenant_id: "default".to_string(),
        transport: "streamable_http".to_string(),
        url: Some("http://localhost:8080/mcp".to_string()),
        active: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    cp.add_upstream(upstream).await.unwrap();

    let upstreams = cp.load_upstreams("default").await.unwrap();
    assert_eq!(upstreams.len(), 1);
    assert_eq!(upstreams[0].server_id, "test-server");
    assert_eq!(
        upstreams[0].url,
        Some("http://localhost:8080/mcp".to_string())
    );
}

#[tokio::test]
async fn test_control_plane_rate_limit_rules() {
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
    cp.add_rate_limit_rule(rule).await.unwrap();

    let rules = cp.load_rate_limit_rules("default").await.unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].rule_id, "test-rule");
}

#[tokio::test]
async fn test_control_plane_multi_tenant_isolation() {
    let cp = InMemoryControlPlane::new();

    // Create tenant A
    let tenant_a = Tenant {
        tenant_id: "tenant-a".to_string(),
        name: "Tenant A".to_string(),
        active: true,
        config: serde_json::json!({}),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    cp.create_tenant(tenant_a).await.unwrap();

    // Create tenant B
    let tenant_b = Tenant {
        tenant_id: "tenant-b".to_string(),
        name: "Tenant B".to_string(),
        active: true,
        config: serde_json::json!({}),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    cp.create_tenant(tenant_b).await.unwrap();

    // Add policy for tenant A
    let policy_a = PolicyEntry {
        policy_id: "policy-a".to_string(),
        tenant_id: "tenant-a".to_string(),
        policy_text: "permit(principal, action, resource);".to_string(),
        enabled: true,
        version: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    cp.update_policy(policy_a).await.unwrap();

    // Add policy for tenant B
    let policy_b = PolicyEntry {
        policy_id: "policy-b".to_string(),
        tenant_id: "tenant-b".to_string(),
        policy_text: "forbid(principal, action, resource);".to_string(),
        enabled: true,
        version: 1,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    cp.update_policy(policy_b).await.unwrap();

    // Verify isolation
    let policies_a = cp.load_policies("tenant-a").await.unwrap();
    assert_eq!(policies_a.len(), 1);
    assert_eq!(policies_a[0].policy_id, "policy-a");

    let policies_b = cp.load_policies("tenant-b").await.unwrap();
    assert_eq!(policies_b.len(), 1);
    assert_eq!(policies_b[0].policy_id, "policy-b");
}

#[tokio::test]
async fn test_rate_limiter_integration_with_control_plane() {
    let cp = InMemoryControlPlane::new();

    // Add rate limit rule to control plane via public API
    let rule = RateLimitRule {
        rule_id: "integration-rule".to_string(),
        tenant_id: "default".to_string(),
        name: "Integration Test Rule".to_string(),
        scope: "global".to_string(),
        scope_value: None,
        algorithm: "token_bucket".to_string(),
        requests_per_window: 10,
        window_seconds: 60,
        burst_allowance: Some(10),
        action: "reject".to_string(),
        priority: 0,
        enabled: true,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    cp.add_rate_limit_rule(rule).await.unwrap();

    // Load rule from control plane and apply to rate limiter
    let rules = cp.load_rate_limit_rules("default").await.unwrap();
    let rate_limiter = InternalTokenBucketRateLimiter::new();

    for rule in rules {
        let telemetry_rule = InternalRateLimitRule {
            rule_id: rule.rule_id,
            scope: rule.scope,
            scope_value: rule.scope_value,
            algorithm: InternalRateLimitAlgorithm::TokenBucket,
            requests_per_window: rule.requests_per_window,
            window_seconds: rule.window_seconds,
            burst_allowance: rule.burst_allowance,
            action: InternalRateLimitAction::Reject,
            priority: rule.priority,
            enabled: rule.enabled,
        };
        rate_limiter.add_rule(telemetry_rule).await.unwrap();
    }

    // Test the rate limiter with the loaded rule
    let key = InternalRateLimitKey::new("global", "test-client");

    // Should allow 10 requests (the rule limit)
    for i in 1..=10 {
        let result = rate_limiter
            .check(&InternalRateLimitKey::new("global", "test"))
            .await
            .unwrap();
        assert!(result.allowed, "Request {} should be allowed", i);
    }

    // 11th should be denied
    let result = rate_limiter
        .check(&InternalRateLimitKey::new("global", "test"))
        .await
        .unwrap();
    assert!(!result.allowed);
    assert_eq!(result.remaining, 0);
}
