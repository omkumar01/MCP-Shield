//! Telemetry and observability layer.
//!
//! Provides Prometheus metrics for real-time observability and an async
//! audit event pipeline (Redpanda → ClickHouse) for forensic logging.

pub mod metrics;
pub mod producer;

pub use metrics::{McpMetrics, install_prometheus_exporter, render_metrics};
pub use producer::{AuditEvent, AuthDecision, BufferingProducer, EventProducer, NoopProducer};
