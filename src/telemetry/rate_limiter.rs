//! Token-bucket rate limiter.
//!
//! Provides rate limiting for MCP requests with support for different
//! algorithms (token bucket, sliding window, fixed window) and scopes.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// A rate limiter key.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct RateLimitKey {
    /// The scope of the rate limit (global, tenant, user, api_key, ip, tool).
    pub scope: String,
    /// The specific value within the scope.
    pub value: String,
}

impl RateLimitKey {
    pub fn new(scope: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            scope: scope.into(),
            value: value.into(),
        }
    }

    pub fn as_string(&self) -> String {
        format!("{}:{}", self.scope, self.value)
    }
}

/// Rate limit algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateLimitAlgorithm {
    /// Token bucket algorithm - allows bursts up to bucket size.
    TokenBucket,
    /// Sliding window algorithm - smooth rate limiting.
    SlidingWindow,
    /// Fixed window algorithm - simple but can allow bursts at window boundaries.
    FixedWindow,
}

/// Action to take when rate limit is exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateLimitAction {
    /// Reject the request with a 429 error.
    Reject,
    /// Throttle the request (delay response).
    Throttle,
    /// Queue the request for later processing.
    Queue,
}

/// A rate limit rule configuration.
#[derive(Debug, Clone)]
pub struct RateLimitRule {
    pub rule_id: String,
    pub scope: String,
    pub scope_value: Option<String>,
    pub algorithm: RateLimitAlgorithm,
    pub requests_per_window: u64,
    pub window_seconds: u32,
    pub burst_allowance: Option<u64>,
    pub action: RateLimitAction,
    pub priority: i32,
    pub enabled: bool,
}

/// The result of a rate limit check.
#[derive(Debug, Clone)]
pub struct RateLimitResult {
    /// Whether the request is allowed.
    pub allowed: bool,
    /// Remaining requests in the current window.
    pub remaining: u64,
    /// Seconds until the limit resets.
    pub reset_after_secs: f64,
    /// The limit that was applied.
    pub limit: u64,
    /// The rule ID that was applied (if any).
    pub rule_id: Option<String>,
    /// The action to take if not allowed.
    pub action: Option<RateLimitAction>,
}

/// Trait for rate limiting.
#[async_trait]
pub trait RateLimiter: Send + Sync {
    /// Check if a request is allowed under the rate limits.
    /// Returns the result and consumes a token if allowed.
    async fn check(&self, key: &RateLimitKey) -> Result<RateLimitResult, RateLimitError>;

    /// Get the current status for a key without consuming a token.
    async fn peek(&self, key: &RateLimitKey) -> Result<RateLimitResult, RateLimitError>;

    /// Reset the rate limit for a specific key.
    async fn reset(&self, key: &RateLimitKey) -> Result<(), RateLimitError>;

    /// Add or update a rate limit rule.
    async fn add_rule(&self, rule: RateLimitRule) -> Result<(), RateLimitError>;

    /// Remove a rate limit rule.
    async fn remove_rule(&self, rule_id: &str) -> Result<(), RateLimitError>;
}

/// Error type for rate limiting.
#[derive(Debug, thiserror::Error)]
pub enum RateLimitError {
    #[error("Rate limit exceeded: {0}")]
    Exceeded(String),

    #[error("Rule not found: {0}")]
    RuleNotFound(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

/// Token bucket state for a single key.
#[derive(Debug, Clone)]
struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
    capacity: u64,
    refill_rate: f64, // tokens per second
}

impl TokenBucket {
    fn new(capacity: u64, refill_rate: f64) -> Self {
        Self {
            tokens: capacity as f64,
            last_refill: Instant::now(),
            capacity,
            refill_rate,
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity as f64);
        self.last_refill = now;
    }

    fn try_consume(&mut self, tokens: f64) -> bool {
        self.refill();
        if self.tokens >= tokens {
            self.tokens -= tokens;
            true
        } else {
            false
        }
    }

    fn remaining(&mut self) -> u64 {
        self.refill();
        self.tokens.floor() as u64
    }

    fn reset_after_secs(&self) -> f64 {
        if self.tokens >= 1.0 {
            0.0
        } else {
            (1.0 - self.tokens) / self.refill_rate
        }
    }
}

/// Token bucket rate limiter implementation.
pub struct TokenBucketRateLimiter {
    buckets: RwLock<HashMap<String, TokenBucket>>,
    rules: RwLock<Vec<RateLimitRule>>,
    default_rule: Option<RateLimitRule>,
}

impl TokenBucketRateLimiter {
    /// Create a new token bucket rate limiter.
    pub fn new() -> Self {
        Self {
            buckets: RwLock::new(HashMap::new()),
            rules: RwLock::new(Vec::new()),
            default_rule: None,
        }
    }

    /// Create with a default rule.
    pub fn with_default_rule(default_rule: RateLimitRule) -> Self {
        Self {
            buckets: RwLock::new(HashMap::new()),
            rules: RwLock::new(Vec::new()),
            default_rule: Some(default_rule),
        }
    }

    /// Get or create a bucket for the given key and rule.
    async fn get_or_create_bucket(&self, key: &str, rule: &RateLimitRule) -> TokenBucket {
        let mut buckets = self.buckets.write().await;
        if let Some(bucket) = buckets.get(key) {
            bucket.clone()
        } else {
            let capacity = rule.burst_allowance.unwrap_or(rule.requests_per_window);
            let refill_rate = rule.requests_per_window as f64 / rule.window_seconds as f64;
            let bucket = TokenBucket::new(capacity, refill_rate);
            buckets.insert(key.to_string(), bucket.clone());
            bucket
        }
    }

    /// Find the highest priority matching rule for a key.
    async fn find_matching_rule(&self, key: &RateLimitKey) -> Option<RateLimitRule> {
        let rules = self.rules.read().await;
        let mut matching: Vec<_> = rules
            .iter()
            .filter(|r| {
                r.enabled
                    && (r.scope == "global"
                        || (r.scope == key.scope
                            && (r.scope_value.is_none()
                                || r.scope_value.as_ref() == Some(&key.value))))
            })
            .cloned()
            .collect();

        matching.sort_by_key(|r| -r.priority); // Higher priority first
        matching.into_iter().next().or(self.default_rule.clone())
    }

    /// Internal method to get or create a bucket and perform an operation on it.
    async fn with_bucket<F, T>(&self, key: &str, rule: &RateLimitRule, f: F) -> T
    where
        F: FnOnce(&mut TokenBucket) -> T,
    {
        let mut buckets = self.buckets.write().await;
        let bucket = buckets.entry(key.to_string()).or_insert_with(|| {
            let capacity = rule.burst_allowance.unwrap_or(rule.requests_per_window);
            let refill_rate = rule.requests_per_window as f64 / rule.window_seconds as f64;
            TokenBucket::new(capacity, refill_rate)
        });
        f(bucket)
    }
}

impl Default for TokenBucketRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl RateLimiter for TokenBucketRateLimiter {
    async fn check(&self, key: &RateLimitKey) -> Result<RateLimitResult, RateLimitError> {
        let rule = self.find_matching_rule(key).await.ok_or_else(|| {
            RateLimitError::InvalidConfig("No matching rate limit rule found".to_string())
        })?;

        let key_str = key.as_string();
        let result = self
            .with_bucket(&key_str, &rule, |bucket| {
                let allowed = bucket.try_consume(1.0);
                let remaining = if allowed { bucket.remaining() } else { 0 };
                let reset_after = bucket.reset_after_secs();
                RateLimitResult {
                    allowed,
                    remaining,
                    reset_after_secs: reset_after,
                    limit: rule.requests_per_window,
                    rule_id: Some(rule.rule_id.clone()),
                    action: if allowed { None } else { Some(rule.action) },
                }
            })
            .await;

        Ok(result)
    }

    async fn peek(&self, key: &RateLimitKey) -> Result<RateLimitResult, RateLimitError> {
        let rule = self.find_matching_rule(key).await.ok_or_else(|| {
            RateLimitError::InvalidConfig("No matching rate limit rule found".to_string())
        })?;

        let key_str = key.as_string();
        let result = self
            .with_bucket(&key_str, &rule, |bucket| {
                let remaining = bucket.remaining();
                let reset_after = bucket.reset_after_secs();
                RateLimitResult {
                    allowed: remaining > 0,
                    remaining,
                    reset_after_secs: reset_after,
                    limit: rule.requests_per_window,
                    rule_id: Some(rule.rule_id.clone()),
                    action: None,
                }
            })
            .await;

        Ok(result)
    }

    async fn reset(&self, key: &RateLimitKey) -> Result<(), RateLimitError> {
        let key_str = key.as_string();
        let mut buckets = self.buckets.write().await;
        buckets.remove(&key_str);
        Ok(())
    }

    async fn add_rule(&self, rule: RateLimitRule) -> Result<(), RateLimitError> {
        let mut rules = self.rules.write().await;
        if rules.iter().any(|r| r.rule_id == rule.rule_id) {
            return Err(RateLimitError::InvalidConfig(format!(
                "Rule with ID '{}' already exists",
                rule.rule_id
            )));
        }
        rules.push(rule);
        // Re-sort by priority (highest first)
        rules.sort_by_key(|r| -r.priority);
        Ok(())
    }

    async fn remove_rule(&self, rule_id: &str) -> Result<(), RateLimitError> {
        let mut rules = self.rules.write().await;
        let pos = rules.iter().position(|r| r.rule_id == rule_id);
        match pos {
            Some(idx) => {
                rules.remove(idx);
                Ok(())
            }
            None => Err(RateLimitError::RuleNotFound(rule_id.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_token_bucket_basic() {
        let limiter = TokenBucketRateLimiter::with_default_rule(RateLimitRule {
            rule_id: "default".to_string(),
            scope: "global".to_string(),
            scope_value: None,
            algorithm: RateLimitAlgorithm::TokenBucket,
            requests_per_window: 10,
            window_seconds: 60,
            burst_allowance: Some(10),
            action: RateLimitAction::Reject,
            priority: 0,
            enabled: true,
        });

        let key = RateLimitKey::new("global", "test");

        // Should allow up to 10 requests
        for i in 1..=10 {
            let result = limiter.check(&key).await.unwrap();
            assert!(result.allowed, "Request {} should be allowed", i);
            assert_eq!(result.remaining, 10 - i);
        }

        // 11th request should be denied
        let result = limiter.check(&key).await.unwrap();
        assert!(!result.allowed);
        assert_eq!(result.remaining, 0);
    }

    #[tokio::test]
    async fn test_token_bucket_refill() {
        let limiter = TokenBucketRateLimiter::with_default_rule(RateLimitRule {
            rule_id: "default".to_string(),
            scope: "global".to_string(),
            scope_value: None,
            algorithm: RateLimitAlgorithm::TokenBucket,
            requests_per_window: 5,
            window_seconds: 1, // 5 requests per second
            burst_allowance: Some(5),
            action: RateLimitAction::Reject,
            priority: 0,
            enabled: true,
        });

        let key = RateLimitKey::new("global", "refill-test");

        // Use all 5 tokens
        for _ in 0..5 {
            let result = limiter.check(&key).await.unwrap();
            assert!(result.allowed);
        }

        // Should be denied
        let result = limiter.check(&key).await.unwrap();
        assert!(!result.allowed);

        // Wait for refill (1.5 seconds)
        tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

        // Should be allowed again
        let result = limiter.check(&key).await.unwrap();
        assert!(result.allowed);
    }

    #[tokio::test]
    async fn test_different_keys_independent() {
        let limiter = TokenBucketRateLimiter::with_default_rule(RateLimitRule {
            rule_id: "default".to_string(),
            scope: "global".to_string(),
            scope_value: None,
            algorithm: RateLimitAlgorithm::TokenBucket,
            requests_per_window: 2,
            window_seconds: 60,
            burst_allowance: Some(2),
            action: RateLimitAction::Reject,
            priority: 0,
            enabled: true,
        });

        let key1 = RateLimitKey::new("global", "user1");
        let key2 = RateLimitKey::new("global", "user2");

        // Use up user1's quota
        limiter.check(&key1).await.unwrap();
        limiter.check(&key1).await.unwrap();
        let result = limiter.check(&key1).await.unwrap();
        assert!(!result.allowed);

        // user2 should still have full quota
        let result = limiter.check(&key2).await.unwrap();
        assert!(result.allowed);
        let result = limiter.check(&key2).await.unwrap();
        assert!(result.allowed);
        let result = limiter.check(&key2).await.unwrap();
        assert!(!result.allowed);
    }

    #[tokio::test]
    async fn test_custom_rules() {
        let limiter = TokenBucketRateLimiter::new();

        // Add a strict rule for specific users
        limiter
            .add_rule(RateLimitRule {
                rule_id: "strict".to_string(),
                scope: "user".to_string(),
                scope_value: Some("bad-user".to_string()),
                algorithm: RateLimitAlgorithm::TokenBucket,
                requests_per_window: 1,
                window_seconds: 60,
                burst_allowance: Some(1),
                action: RateLimitAction::Reject,
                priority: 10, // Higher priority than default
                enabled: true,
            })
            .await
            .unwrap();

        // Add a generous default rule
        limiter
            .add_rule(RateLimitRule {
                rule_id: "default".to_string(),
                scope: "global".to_string(),
                scope_value: None,
                algorithm: RateLimitAlgorithm::TokenBucket,
                requests_per_window: 100,
                window_seconds: 60,
                burst_allowance: Some(100),
                action: RateLimitAction::Reject,
                priority: 0,
                enabled: true,
            })
            .await
            .unwrap();

        let bad_user = RateLimitKey::new("user", "bad-user");
        let good_user = RateLimitKey::new("user", "good-user");

        // Bad user gets strict limit
        let result = limiter.check(&bad_user).await.unwrap();
        assert!(result.allowed);
        let result = limiter.check(&bad_user).await.unwrap();
        assert!(!result.allowed);

        // Good user gets generous limit
        for _ in 0..50 {
            let result = limiter.check(&good_user).await.unwrap();
            assert!(result.allowed);
        }
    }

    #[tokio::test]
    async fn test_peek_does_not_consume() {
        let limiter = TokenBucketRateLimiter::with_default_rule(RateLimitRule {
            rule_id: "default".to_string(),
            scope: "global".to_string(),
            scope_value: None,
            algorithm: RateLimitAlgorithm::TokenBucket,
            requests_per_window: 2,
            window_seconds: 60,
            burst_allowance: Some(2),
            action: RateLimitAction::Reject,
            priority: 0,
            enabled: true,
        });

        let key = RateLimitKey::new("global", "peek-test");

        // Peek should show 2 available
        let result = limiter.peek(&key).await.unwrap();
        assert_eq!(result.remaining, 2);
        assert!(result.allowed);

        // Peek again should still show 2
        let result = limiter.peek(&key).await.unwrap();
        assert_eq!(result.remaining, 2);

        // Actual check should consume
        let result = limiter.check(&key).await.unwrap();
        assert!(result.allowed);
        assert_eq!(result.remaining, 1);
    }

    #[tokio::test]
    async fn test_reset() {
        let limiter = TokenBucketRateLimiter::with_default_rule(RateLimitRule {
            rule_id: "default".to_string(),
            scope: "global".to_string(),
            scope_value: None,
            algorithm: RateLimitAlgorithm::TokenBucket,
            requests_per_window: 2,
            window_seconds: 60,
            burst_allowance: Some(2),
            action: RateLimitAction::Reject,
            priority: 0,
            enabled: true,
        });

        let key = RateLimitKey::new("global", "reset-test");

        // Use up quota
        limiter.check(&key).await.unwrap();
        limiter.check(&key).await.unwrap();
        let result = limiter.check(&key).await.unwrap();
        assert!(!result.allowed);

        // Reset
        limiter.reset(&key).await.unwrap();

        // Should be allowed again
        let result = limiter.check(&key).await.unwrap();
        assert!(result.allowed);
    }

    #[tokio::test]
    async fn test_remove_rule() {
        let limiter = TokenBucketRateLimiter::new();

        limiter
            .add_rule(RateLimitRule {
                rule_id: "test-rule".to_string(),
                scope: "global".to_string(),
                scope_value: None,
                algorithm: RateLimitAlgorithm::TokenBucket,
                requests_per_window: 10,
                window_seconds: 60,
                burst_allowance: Some(10),
                action: RateLimitAction::Reject,
                priority: 0,
                enabled: true,
            })
            .await
            .unwrap();

        // Rule should work
        let key = RateLimitKey::new("global", "test");
        let result = limiter.check(&key).await.unwrap();
        assert!(result.allowed);

        // Remove rule
        limiter.remove_rule("test-rule").await.unwrap();

        // Should fall back to no rule (need default rule for this to work)
        // Actually without default rule, it will error
        let result = limiter.check(&key).await;
        assert!(result.is_err());
    }
}
