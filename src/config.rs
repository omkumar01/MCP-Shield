//! Typed configuration for MCP-Shield.
//!
//! Configuration is loaded from TOML files, environment variables, and CLI arguments.
//! The config module provides a strongly-typed struct that is validated at startup.

use serde::Deserialize;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;

/// Top-level configuration for MCP-Shield.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Server settings (bind address, logging, CORS).
    #[serde(default)]
    pub server: ServerConfig,

    /// Authentication and authorization settings.
    #[serde(default)]
    pub auth: AuthConfig,

    /// Upstream proxy settings.
    #[serde(default)]
    pub proxy: ProxyConfig,

    /// Tool registry namespace settings.
    #[serde(default)]
    pub registry: RegistryConfig,

    /// Telemetry settings (metrics, audit logging).
    #[serde(default)]
    pub telemetry: TelemetryConfig,
}

/// Server configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Listen address for the HTTP server.
    #[serde(default = "default_bind_addr")]
    pub bind_addr: SocketAddr,

    /// Log level: "trace", "debug", "info", "warn", "error".
    #[serde(default = "default_log_level")]
    pub log_level: String,

    /// Whether to enable structured JSON logging.
    #[serde(default = "default_true")]
    pub json_logging: bool,

    /// Allowed CORS origins (empty = allow all in dev mode).
    #[serde(default)]
    pub cors_origins: Vec<String>,

    /// Whether to enable the stdio transport (for local CLI usage).
    #[serde(default)]
    pub enable_stdio: bool,

    /// Whether to enable the Streamable HTTP transport.
    #[serde(default = "default_true")]
    pub enable_http: bool,

    /// Whether to enable the legacy SSE transport.
    #[serde(default = "default_true")]
    pub enable_sse: bool,

    /// Server info for MCP initialize response.
    #[serde(default)]
    pub server_info: ServerInfo,
}

/// Server info returned during MCP initialization.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerInfo {
    /// Gateway name advertised to MCP clients.
    #[serde(default = "default_server_name")]
    pub name: String,

    /// Gateway version advertised to MCP clients.
    #[serde(default = "default_server_version")]
    pub version: String,

    /// Instructions for LLM clients.
    #[serde(default)]
    pub instructions: String,
}

/// Authentication and authorization configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    /// Whether authentication is required (disable for local dev).
    #[serde(default)]
    pub enabled: bool,

    /// JWT secret for HMAC signing (mutually exclusive with jwks_url).
    #[serde(default)]
    pub jwt_secret: Option<String>,

    /// JWKS URL for RSA/EC key rotation (mutually exclusive with jwt_secret).
    #[serde(default)]
    pub jwks_url: Option<String>,

    /// Expected token issuer claim.
    #[serde(default)]
    pub issuer: Option<String>,

    /// Expected audience claim.
    #[serde(default)]
    pub audience: Option<String>,

    /// URL for the Protected Resource Metadata (PRM) document.
    #[serde(default = "default_prm_url")]
    pub prm_url: String,

    /// Authorization server URL for OIDC discovery.
    #[serde(default)]
    pub authorization_server: Option<String>,

    /// Required OAuth 2.1 scopes for MCP access.
    #[serde(default = "default_scopes")]
    pub required_scopes: Vec<String>,
}

/// Proxy configuration for upstream MCP servers.
#[derive(Debug, Clone, Deserialize)]
pub struct ProxyConfig {
    /// Map of named upstream servers.
    #[serde(default)]
    pub upstreams: HashMap<String, UpstreamConfig>,

    /// Request timeout per upstream call (in seconds).
    #[serde(default = "default_timeout")]
    pub request_timeout_secs: u64,

    /// Maximum concurrent requests to any single upstream.
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_requests: usize,
}

/// Configuration for a single upstream MCP server.
#[derive(Debug, Clone, Deserialize)]
pub struct UpstreamConfig {
    /// Transport type: "streamable_http", "sse", or "stdio".
    #[serde(default = "default_transport")]
    pub transport: String,

    /// URL for HTTP-based transports.
    #[serde(default)]
    pub url: Option<String>,

    /// Command and args for stdio transport.
    #[serde(default)]
    pub command: Option<String>,

    /// Arguments for stdio command.
    #[serde(default)]
    pub args: Vec<String>,

    /// Environment variables for stdio transport.
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Whether this upstream uses the built-in echo test server.
    #[serde(default)]
    pub echo_server: bool,
}

/// Tool registry configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct RegistryConfig {
    /// Allowed namespace prefixes (empty = allow all).
    #[serde(default)]
    pub allowed_prefixes: Vec<String>,

    /// Whether to enforce strict reverse-DNS prefix validation.
    #[serde(default = "default_true")]
    pub enforce_prefix_format: bool,
}

/// Telemetry configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct TelemetryConfig {
    /// Prometheus metrics endpoint path.
    #[serde(default = "default_metrics_path")]
    pub metrics_path: String,

    /// Metrics listen address (separate from main server).
    #[serde(default = "default_metrics_addr")]
    pub metrics_addr: SocketAddr,

    /// Redpanda/Kafka bootstrap servers for audit logging.
    #[serde(default)]
    pub kafka_bootstrap_servers: Vec<String>,

    /// Kafka topic for audit events.
    #[serde(default = "default_kafka_topic")]
    pub kafka_topic: String,

    /// ClickHouse URL for long-term forensic storage.
    #[serde(default)]
    pub clickhouse_url: Option<String>,
}

// ── Default value functions ─────────────────────────────────────────

fn default_bind_addr() -> SocketAddr {
    "0.0.0.0:8080".parse().unwrap()
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_true() -> bool {
    true
}

fn default_server_name() -> String {
    "mcp-shield".to_string()
}

fn default_server_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn default_prm_url() -> String {
    "/.well-known/oauth-protected-resource".to_string()
}

fn default_scopes() -> Vec<String> {
    vec!["mcp:tools:read".to_string(), "mcp:tools:call".to_string()]
}

fn default_timeout() -> u64 {
    30
}

fn default_max_concurrent() -> usize {
    100
}

fn default_transport() -> String {
    "streamable_http".to_string()
}

fn default_metrics_path() -> String {
    "/metrics".to_string()
}

fn default_metrics_addr() -> SocketAddr {
    "0.0.0.0:9090".parse().unwrap()
}

fn default_kafka_topic() -> String {
    "mcp-shield-audit".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            auth: AuthConfig::default(),
            proxy: ProxyConfig::default(),
            registry: RegistryConfig::default(),
            telemetry: TelemetryConfig::default(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: default_bind_addr(),
            log_level: default_log_level(),
            json_logging: true,
            cors_origins: vec![],
            enable_stdio: false,
            enable_http: true,
            enable_sse: true,
            server_info: ServerInfo::default(),
        }
    }
}

impl Default for ServerInfo {
    fn default() -> Self {
        Self {
            name: default_server_name(),
            version: default_server_version(),
            instructions: String::new(),
        }
    }
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            jwt_secret: None,
            jwks_url: None,
            issuer: None,
            audience: None,
            prm_url: default_prm_url(),
            authorization_server: None,
            required_scopes: default_scopes(),
        }
    }
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            upstreams: HashMap::new(),
            request_timeout_secs: default_timeout(),
            max_concurrent_requests: default_max_concurrent(),
        }
    }
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            allowed_prefixes: vec![],
            enforce_prefix_format: true,
        }
    }
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            metrics_path: default_metrics_path(),
            metrics_addr: default_metrics_addr(),
            kafka_bootstrap_servers: vec![],
            kafka_topic: default_kafka_topic(),
            clickhouse_url: None,
        }
    }
}

impl Config {
    /// Load configuration from a TOML file.
    pub fn from_file(path: &PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    /// Load configuration with environment variable overrides.
    ///
    /// Environment variables take the form `MCP_SHIELD_<SECTION>_<FIELD>`
    /// e.g., `MCP_SHIELD_SERVER_BIND_ADDR=0.0.0.0:9090`
    pub fn load_with_env(mut self) -> Self {
        // Override bind address from env
        if let Ok(addr) = std::env::var("MCP_SHIELD_SERVER_BIND_ADDR") {
            if let Ok(parsed) = addr.parse() {
                self.server.bind_addr = parsed;
            }
        }

        // Override log level from env
        if let Ok(level) = std::env::var("MCP_SHIELD_SERVER_LOG_LEVEL") {
            self.server.log_level = level;
        }

        // Override auth enabled flag
        if let Ok(val) = std::env::var("MCP_SHIELD_AUTH_ENABLED") {
            self.auth.enabled = val == "true" || val == "1";
        }

        // Override JWT secret from env
        if let Ok(secret) = std::env::var("MCP_SHIELD_AUTH_JWT_SECRET") {
            self.auth.jwt_secret = Some(secret);
        }

        // Override JWKS URL from env
        if let Ok(url) = std::env::var("MCP_SHIELD_AUTH_JWKS_URL") {
            self.auth.jwks_url = Some(url);
        }

        self
    }

    /// Validate the configuration, returning all issues found.
    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();

        if self.auth.enabled {
            if self.auth.jwt_secret.is_none() && self.auth.jwks_url.is_none() {
                issues.push(
                    "Auth is enabled but neither jwt_secret nor jwks_url is configured".to_string(),
                );
            }
            if self.auth.jwt_secret.is_some() && self.auth.jwks_url.is_some() {
                issues.push(
                    "Both jwt_secret and jwks_url are set; only one should be used".to_string(),
                );
            }
        }

        if self.proxy.upstreams.is_empty() && !self.server.enable_stdio {
            // Not an error — the gateway can still serve as a policy enforcement point
            tracing::warn!("No upstream servers configured and stdio is disabled");
        }

        issues
    }
}
