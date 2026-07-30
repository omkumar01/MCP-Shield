//! Async audit event producer for Redpanda/Kafka.
//!
//! Publishes sanitized JSON-RPC payloads and authorization decisions
//! to a Kafka topic without blocking the main request thread.
//!
//! **Phase 2 — STUB.** This module defines the trait contract for the
//! audit pipeline. Full implementation will use the `rdkafka` crate
//! to publish events to Redpanda, with ClickHouse consumers ingesting
//! the streams for long-term forensic auditing.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

/// An audit event published to the telemetry pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Unique event ID.
    pub event_id: String,

    /// Event timestamp (RFC 3339).
    pub timestamp: String,

    /// The MCP method (e.g., "tools/call").
    pub method: String,

    /// The request ID (correlation).
    pub request_id: Option<String>,

    /// The session ID.
    pub session_id: Option<String>,

    /// The authenticated principal (client ID).
    pub principal: Option<String>,

    /// Granted scopes at the time of the request.
    pub scopes: Vec<String>,

    /// The authorization decision.
    pub decision: AuthDecision,

    /// The sanitized request payload (secrets redacted).
    pub request_payload: Option<Value>,

    /// The sanitized response payload (secrets redacted).
    pub response_payload: Option<Value>,

    /// Error code if the request failed.
    pub error_code: Option<i64>,

    /// Request duration in milliseconds.
    pub duration_ms: u64,

    /// Transport used ("stdio", "http", "sse").
    pub transport: String,
}

/// Authorization decision for an audit event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum AuthDecision {
    /// Request was allowed.
    Allow,
    /// Request was denied by scope policy.
    Deny,
    /// Request was denied by Cedar policy.
    DenyPolicy,
    /// Request was blocked by a guardrail (ePCA, session lock).
    Block,
}

/// Trait for publishing audit events to an async pipeline.
///
/// Implementations must be non-blocking. The gateway calls `publish_audit_event`
/// fire-and-forget from the main request thread.
#[async_trait]
pub trait EventProducer: Send + Sync {
    /// Publish an audit event to the pipeline.
    ///
    /// This method should never block the caller. If the pipeline is
    /// unavailable, the event should be buffered or dropped with a warning.
    async fn publish_audit_event(&self, event: AuditEvent) -> Result<(), ProducerError>;

    /// Flush any buffered events.
    async fn flush(&self) -> Result<(), ProducerError>;
}

/// Error type for event producers.
#[derive(Debug, thiserror::Error)]
pub enum ProducerError {
    #[error("Kafka error: {0}")]
    Kafka(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Channel full: events are being dropped")]
    ChannelFull,

    #[error("Producer not connected")]
    NotConnected,
}

/// A no-op producer for development and testing.
///
/// Logs events to stderr instead of publishing to Kafka.
pub struct NoopProducer;

#[async_trait]
impl EventProducer for NoopProducer {
    async fn publish_audit_event(&self, event: AuditEvent) -> Result<(), ProducerError> {
        tracing::debug!(event = ?event, "Audit event (noop producer)");
        Ok(())
    }

    async fn flush(&self) -> Result<(), ProducerError> {
        Ok(())
    }
}

/// A buffering producer that holds events in memory.
///
/// Useful for testing and as a fallback when Kafka is unavailable.
pub struct BufferingProducer {
    buffer: tokio::sync::Mutex<Vec<AuditEvent>>,
}

impl BufferingProducer {
    /// Create a new buffering producer.
    pub fn new() -> Self {
        Self {
            buffer: tokio::sync::Mutex::new(Vec::new()),
        }
    }

    /// Drain all buffered events.
    pub async fn drain(&self) -> Vec<AuditEvent> {
        let mut buf = self.buffer.lock().await;
        std::mem::take(&mut *buf)
    }
}

#[async_trait]
impl EventProducer for BufferingProducer {
    async fn publish_audit_event(&self, event: AuditEvent) -> Result<(), ProducerError> {
        let mut buf = self.buffer.lock().await;
        buf.push(event);
        Ok(())
    }

    async fn flush(&self) -> Result<(), ProducerError> {
        Ok(())
    }
}

impl Default for BufferingProducer {
    fn default() -> Self {
        Self::new()
    }
}

/// Kafka/Redpanda producer using the `rdkafka` crate.
///
/// **Phase 2 — not yet implemented.** This is the production producer
/// that will publish to Redpanda. The `rdkafka` crate requires the
/// librdkafka C library at build time.
pub struct KafkaProducer {
    bootstrap_servers: Vec<String>,
    topic: String,
}

impl KafkaProducer {
    /// Create a new Kafka producer configuration.
    ///
    /// The producer is not connected until `connect()` is called.
    pub fn new(bootstrap_servers: Vec<String>, topic: String) -> Self {
        Self {
            bootstrap_servers,
            topic,
        }
    }

    /// Connect to the Kafka cluster.
    ///
    /// **TODO (Phase 2):** Implement using `rdkafka::producer::FutureProducer`.
    pub async fn connect(&self) -> Result<(), ProducerError> {
        tracing::warn!(
            servers = ?self.bootstrap_servers,
            topic = %self.topic,
            "KafkaProducer::connect() is a stub (Phase 2)"
        );
        Err(ProducerError::NotConnected)
    }
}
