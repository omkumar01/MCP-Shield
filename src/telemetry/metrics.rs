//! Prometheus metrics for MCP-Shield.
//!
//! Records JSON-RPC throughput, latency, and blocked request rates.
//! Exposed at the `/metrics` endpoint for scraping by Prometheus.

use metrics::{counter, gauge, histogram};
use std::time::Duration;

/// Metrics recorder for MCP-Shield.
///
/// Wraps the `metrics` facade to provide a typed interface for recording
/// the gateway's operational metrics.
#[derive(Debug, Clone)]
pub struct McpMetrics;

impl McpMetrics {
    /// Create a new metrics recorder.
    pub fn new() -> Self {
        McpMetrics
    }

    /// Record a completed request.
    ///
    /// - `method`: The MCP method (e.g., "tools/call")
    /// - `transport`: The transport ("stdio", "http", "sse")
    /// - `status`: "success" or "error"
    /// - `duration`: Request processing time
    pub fn record_request(&self, method: &str, transport: &str, status: &str, duration: Duration) {
        counter!(
            "mcp_requests_total",
            "method" => method.to_string(),
            "transport" => transport.to_string(),
            "status" => status.to_string(),
        )
        .increment(1);

        histogram!(
            "mcp_request_duration_seconds",
            "method" => method.to_string(),
        )
        .record(duration.as_secs_f64());
    }

    /// Increment the active session count.
    pub fn increment_active_sessions(&self) {
        gauge!("mcp_active_sessions").increment(1.0);
    }

    /// Decrement the active session count.
    pub fn decrement_active_sessions(&self) {
        gauge!("mcp_active_sessions").decrement(1.0);
    }

    /// Record an authentication failure.
    pub fn increment_auth_failure(&self, reason: &str) {
        counter!(
            "mcp_auth_failures_total",
            "reason" => reason.to_string(),
        )
        .increment(1);
    }

    /// Record a validation failure.
    pub fn increment_validation_failure(&self, reason: &str) {
        counter!(
            "mcp_validation_failures_total",
            "reason" => reason.to_string(),
        )
        .increment(1);
    }

    /// Record a blocked request (scope denial, registry collision, etc.).
    pub fn increment_blocked_request(&self, reason: &str) {
        counter!(
            "mcp_blocked_requests_total",
            "reason" => reason.to_string(),
        )
        .increment(1);
    }

    /// Record a tool registration.
    pub fn record_tool_registration(&self, prefix: &str) {
        counter!(
            "mcp_tool_registrations_total",
            "prefix" => prefix.to_string(),
        )
        .increment(1);
        gauge!("mcp_registered_tools").increment(1.0);
    }

    /// Record a tool deregistration.
    pub fn record_tool_deregistration(&self) {
        gauge!("mcp_registered_tools").decrement(1.0);
    }

    /// Record an upstream request.
    pub fn record_upstream_request(&self, server_id: &str, status: &str) {
        counter!(
            "mcp_upstream_requests_total",
            "server" => server_id.to_string(),
            "status" => status.to_string(),
        )
        .increment(1);
    }

    /// Set the current number of active connections.
    pub fn set_active_connections(&self, count: i64) {
        gauge!("mcp_active_connections").set(count as f64);
    }
}

impl Default for McpMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Install the Prometheus metrics exporter.
///
/// This sets up the metrics recorder so that metrics can be scraped
/// from the configured HTTP endpoint.
pub fn install_prometheus_exporter() -> Result<(), Box<dyn std::error::Error>> {
    use metrics_exporter_prometheus::PrometheusBuilder;

    PrometheusBuilder::new()
        .install_recorder()
        .map(|_| ())
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
}

/// Render the current metrics in Prometheus exposition format.
pub fn render_metrics() -> String {
    // The PrometheusBuilder::install_recorder() returns a handle
    // that can render metrics. For simplicity in this scaffold,
    // we rely on the metrics-exporter-prometheus HTTP handler.
    // In main.rs we use the handle's render() method.
    String::from("# Metrics handled by metrics-exporter-prometheus\n")
}
