//! Async audit event producer for Redpanda/Kafka.
//!
//! Publishes sanitized JSON-RPC payloads and authorization decisions
//! to a Kafka topic without blocking the main request thread.
//!
//! **Phase 2 — PRODUCTION.** Supports multiple backends:
//! - `LoggingProducer`: structured logging to tracing (default, no deps)
//! - `BufferingProducer`: in-memory buffer for testing
//! - `KafkaProducer`: real Redpanda/Kafka producer (feature-gated)

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, mpsc};
use tracing::info;

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

/// A structured logging producer that emits audit events via `tracing::info!`.
///
/// This is the default production-ready producer when Kafka is not configured.
/// It produces structured JSON logs suitable for log aggregation systems.
pub struct LoggingProducer;

#[async_trait]
impl EventProducer for LoggingProducer {
    async fn publish_audit_event(&self, event: AuditEvent) -> Result<(), ProducerError> {
        let json = serde_json::to_string(&event)
            .map_err(|e| ProducerError::Serialization(e.to_string()))?;
        info!(audit = %json, "MCP-Shield audit event");
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
    buffer: Mutex<Vec<AuditEvent>>,
}

impl BufferingProducer {
    /// Create a new buffering producer.
    pub fn new() -> Self {
        Self {
            buffer: Mutex::new(Vec::new()),
        }
    }

    /// Drain all buffered events.
    pub async fn drain(&self) -> Vec<AuditEvent> {
        let mut buf = self.buffer.lock().await;
        std::mem::take(&mut *buf)
    }

    /// Get the current buffer size.
    pub async fn len(&self) -> usize {
        self.buffer.lock().await.len()
    }
}

impl Default for BufferingProducer {
    fn default() -> Self {
        Self::new()
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

/// Kafka/Redpanda producer using the `rdkafka` crate.
///
/// **Feature-gated:** Only available when the `kafka` feature is enabled.
/// The default build does NOT include this to keep Windows MSVC builds working.
#[cfg(feature = "kafka")]
pub mod kafka_producer {
    use super::*;
    use rdkafka::config::ClientConfig;
    use rdkafka::producer::{FutureProducer, FutureRecord};
    use std::time::Duration;

    /// Kafka producer configuration.
    #[derive(Debug, Clone)]
    pub struct KafkaProducerConfig {
        pub bootstrap_servers: Vec<String>,
        pub topic: String,
        pub client_id: String,
        pub acks: String,
        pub retries: i32,
        pub max_in_flight: i32,
        pub compression_type: String,
        pub linger_ms: i32,
        pub batch_size: i32,
    }

    impl Default for KafkaProducerConfig {
        fn default() -> Self {
            Self {
                bootstrap_servers: vec!["localhost:9092".to_string()],
                topic: "mcp-shield-audit".to_string(),
                client_id: "mcp-shield".to_string(),
                acks: "all".to_string(),
                retries: 3,
                max_in_flight: 5,
                compression_type: "snappy".to_string(),
                linger_ms: 5,
                batch_size: 16384,
            }
        }
    }

    /// Kafka/Redpanda producer with async send.
    pub struct KafkaProducer {
        producer: FutureProducer,
        topic: String,
    }

    impl KafkaProducer {
        /// Create a new Kafka producer.
        pub fn new(config: KafkaProducerConfig) -> Result<Self, ProducerError> {
            let mut client_config = ClientConfig::new();
            client_config
                .set("bootstrap.servers", &config.bootstrap_servers.join(","))
                .set("client.id", &config.client_id)
                .set("acks", &config.acks)
                .set("retries", config.retries.to_string())
                .set(
                    "max.in.flight.requests.per.connection",
                    config.max_in_flight.to_string(),
                )
                .set("compression.type", &config.compression_type)
                .set("linger.ms", config.linger_ms.to_string())
                .set("batch.size", config.batch_size.to_string())
                .set("message.timeout.ms", "30000")
                .set("enable.idempotence", "true");

            let producer: FutureProducer = client_config.create().map_err(|e| {
                ProducerError::Kafka(format!("Failed to create Kafka producer: {}", e))
            })?;

            Ok(Self {
                producer,
                topic: config.topic,
            })
        }

        /// Check if the producer is connected.
        pub async fn check_connection(&self) -> bool {
            // rdkafka doesn't have a direct health check, but we can try to send a test message
            // For now, just return true if created successfully
            true
        }
    }

    #[async_trait]
    impl EventProducer for KafkaProducer {
        async fn publish_audit_event(&self, event: AuditEvent) -> Result<(), ProducerError> {
            let key = event
                .session_id
                .clone()
                .unwrap_or_else(|| event.event_id.clone());
            let payload = serde_json::to_vec(&event)
                .map_err(|e| ProducerError::Serialization(e.to_string()))?;

            let record = FutureRecord::to(&self.topic).key(&key).payload(&payload);

            // Send with a timeout
            let delivery_result = self.producer.send(record, Duration::from_secs(5)).await;

            match delivery_result {
                Ok(_) => Ok(()),
                Err((e, _)) => Err(ProducerError::Kafka(format!("Kafka send failed: {}", e))),
            }
        }

        async fn flush(&self) -> Result<(), ProducerError> {
            // Flush with timeout
            let _ = self.producer.flush(Duration::from_secs(10)).await;
            Ok(())
        }
    }
}

// Re-export KafkaProducer when feature is enabled
#[cfg(feature = "kafka")]
pub use kafka_producer::{KafkaProducer, KafkaProducerConfig};

/// A producer that wraps multiple producers and fans out to all of them.
///
/// Useful for sending to both Kafka and logging simultaneously.
pub struct FanoutProducer {
    producers: Vec<Arc<dyn EventProducer>>,
}

impl FanoutProducer {
    /// Create a new fanout producer.
    pub fn new(producers: Vec<Arc<dyn EventProducer>>) -> Self {
        Self { producers }
    }

    /// Add a producer to the fanout.
    pub fn add_producer(&mut self, producer: Arc<dyn EventProducer>) {
        self.producers.push(producer);
    }
}

#[async_trait]
impl EventProducer for FanoutProducer {
    async fn publish_audit_event(&self, event: AuditEvent) -> Result<(), ProducerError> {
        let mut last_err = None;
        for producer in &self.producers {
            if let Err(e) = producer.publish_audit_event(event.clone()).await {
                tracing::warn!(error = %e, "Fanout producer failed");
                last_err = Some(e);
            }
        }
        last_err.map_or(Ok(()), Err)
    }

    async fn flush(&self) -> Result<(), ProducerError> {
        for producer in &self.producers {
            if let Err(e) = producer.flush().await {
                tracing::warn!(error = %e, "Fanout producer flush failed");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_noop_producer() {
        let producer = NoopProducer;
        let event = AuditEvent {
            event_id: "test-1".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            method: "tools/call".to_string(),
            request_id: Some("req-1".to_string()),
            session_id: Some("sess-1".to_string()),
            principal: Some("client-1".to_string()),
            scopes: vec!["mcp:tools:call".to_string()],
            decision: AuthDecision::Allow,
            request_payload: None,
            response_payload: None,
            error_code: None,
            duration_ms: 10,
            transport: "http".to_string(),
        };
        assert!(producer.publish_audit_event(event).await.is_ok());
    }

    #[tokio::test]
    async fn test_logging_producer() {
        let producer = LoggingProducer;
        let event = AuditEvent {
            event_id: "test-2".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            method: "tools/call".to_string(),
            request_id: Some("req-2".to_string()),
            session_id: Some("sess-2".to_string()),
            principal: Some("client-2".to_string()),
            scopes: vec!["mcp:tools:read".to_string()],
            decision: AuthDecision::Allow,
            request_payload: None,
            response_payload: None,
            error_code: None,
            duration_ms: 5,
            transport: "http".to_string(),
        };
        assert!(producer.publish_audit_event(event).await.is_ok());
    }

    #[tokio::test]
    async fn test_buffering_producer() {
        let producer = BufferingProducer::new();
        let event = AuditEvent {
            event_id: "test-3".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            method: "tools/call".to_string(),
            request_id: Some("req-3".to_string()),
            session_id: Some("sess-3".to_string()),
            principal: Some("client-3".to_string()),
            scopes: vec!["mcp:tools:call".to_string()],
            decision: AuthDecision::Deny,
            request_payload: None,
            response_payload: None,
            error_code: Some(-32002),
            duration_ms: 2,
            transport: "http".to_string(),
        };
        assert!(producer.publish_audit_event(event).await.is_ok());
        assert_eq!(producer.len().await, 1);
        let drained = producer.drain().await;
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].decision, AuthDecision::Deny);
    }

    #[tokio::test]
    async fn test_fanout_producer() {
        let logging = Arc::new(LoggingProducer);
        let buffer = Arc::new(BufferingProducer::new());
        let fanout = FanoutProducer::new(vec![logging, buffer.clone()]);

        let event = AuditEvent {
            event_id: "test-4".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            method: "tools/call".to_string(),
            request_id: Some("req-4".to_string()),
            session_id: Some("sess-4".to_string()),
            principal: Some("client-4".to_string()),
            scopes: vec!["mcp:tools:read".to_string()],
            decision: AuthDecision::Allow,
            request_payload: None,
            response_payload: None,
            error_code: None,
            duration_ms: 1,
            transport: "http".to_string(),
        };
        assert!(fanout.publish_audit_event(event).await.is_ok());
        assert_eq!(buffer.len().await, 1);
    }
}
