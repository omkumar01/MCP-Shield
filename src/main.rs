//! MCP-Shield gateway server entry point.
//!
//! Bootstraps the gateway, initializes the router and transport listeners,
//! and serves requests over the configured transports.

use axum::routing::{delete, get, post};
use axum::Router;
use mcp_shield::{
    auth::ScopeEnforcer,
    config::Config,
    gateway::{McpRouter, ToolRegistry, UpstreamProxy, UpstreamServer, UpstreamTransport},
    telemetry::{install_prometheus_exporter, McpMetrics},
    transport::{SseState, StdioTransport, StreamableHttpState},
    EchoServer,
};
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ── 1. Load configuration ─────────────────────────────────────────
    let config_path = std::env::var("MCP_SHIELD_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("config/default.toml"));

    let config = if config_path.exists() {
        Config::from_file(&config_path)?.load_with_env()
    } else {
        Config::default().load_with_env()
    };

    for issue in config.validate() {
        tracing::warn!(issue = %issue, "Configuration issue");
    }

    // ── 2. Initialize tracing ──────────────────────────────────────────
    init_tracing(&config.server.log_level, config.server.json_logging);

    tracing::info!(
        version = mcp_shield::VERSION,
        protocol = mcp_shield::MCP_PROTOCOL_VERSION,
        bind_addr = %config.server.bind_addr,
        "Starting MCP-Shield gateway"
    );

    // ── 3. Initialize metrics ──────────────────────────────────────────
    install_prometheus_exporter()?;
    let metrics = Arc::new(McpMetrics::new());

    // ── 4. Initialize the tool registry ────────────────────────────────
    let registry = Arc::new(ToolRegistry::with_config(
        config.registry.enforce_prefix_format,
        config.registry.allowed_prefixes.clone(),
    ));

    // Register echo server tools if configured or no upstreams
    if config.proxy.upstreams.is_empty() || has_echo_upstream(&config) {
        tracing::info!("Registering built-in echo test server tools");
        for tool in EchoServer::list_tools() {
            if let Err(e) = registry.register_tool(tool, "echo").await {
                tracing::error!(error = %e, "Failed to register echo tool");
            }
        }
    }

    // ── 5. Initialize the upstream proxy ───────────────────────────────
    let proxy = Arc::new(UpstreamProxy::new(
        config.proxy.request_timeout_secs,
        config.proxy.max_concurrent_requests,
    ));

    // Register the echo server as a default upstream
    proxy
        .register_server(UpstreamServer {
            id: "echo".to_string(),
            transport: UpstreamTransport::StreamableHttp,
            url: None,
            is_echo: true,
        })
        .await;

    // Register configured upstreams
    for (id, upstream) in &config.proxy.upstreams {
        let transport = match upstream.transport.as_str() {
            "sse" => UpstreamTransport::Sse,
            "stdio" => UpstreamTransport::Stdio,
            _ => UpstreamTransport::StreamableHttp,
        };
        proxy
            .register_server(UpstreamServer {
                id: id.clone(),
                transport,
                url: upstream.url.clone(),
                is_echo: upstream.echo_server,
            })
            .await;
    }

    // ── 6. Initialize the router ───────────────────────────────────────
    let router = Arc::new(McpRouter::new(
        registry.clone(),
        proxy.clone(),
        metrics.clone(),
        config.server.server_info.name.clone(),
        config.server.server_info.version.clone(),
    ));

    // ── 7. Scope enforcer (Phase 1: permissive; Phase 2: per-request JWT) ─
    let scope_enforcer = if config.auth.enabled {
        tracing::info!("Authentication is enabled");
        ScopeEnforcer::permissive()
    } else {
        tracing::warn!("Authentication is DISABLED — gateway runs in open mode");
        ScopeEnforcer::permissive()
    };

    // ── 8. Route based on configured transport ─────────────────────────
    if config.server.enable_http {
        run_http_server(router, scope_enforcer, &config).await?;
    } else if config.server.enable_stdio {
        let stdio = StdioTransport::new(router);
        tracing::info!("Starting stdio transport");
        stdio.run(scope_enforcer).await?;
    } else {
        tracing::warn!("No transports enabled. Set enable_http or enable_stdio in config.");
    }

    tracing::info!("MCP-Shield shutdown complete");
    Ok(())
}

/// Run the HTTP server with Streamable HTTP and optional SSE transports.
async fn run_http_server(
    router: Arc<McpRouter>,
    scope_enforcer: ScopeEnforcer,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    let cors = build_cors_layer(config);

    let streamable_state = Arc::new(StreamableHttpState::new(
        router.clone(),
        scope_enforcer.clone(),
    ));

    // Streamable HTTP routes (state: StreamableHttpState)
    let streamable_router = Router::new()
        .route(
            "/mcp",
            post(mcp_shield::transport::streamable_http::handle_mcp_post).delete(
                mcp_shield::transport::streamable_http::handle_mcp_delete,
            ),
        )
        .with_state(streamable_state);

    // Build the app
    let mut app = Router::new();

    // Merge streamable HTTP routes
    app = app.merge(streamable_router);

    // Legacy SSE transport (separate state)
    if config.server.enable_sse {
        let sse_state = Arc::new(SseState::new(router.clone(), scope_enforcer.clone()));
        let sse_router = Router::new()
            .route(
                "/sse",
                get(mcp_shield::transport::sse::handle_sse_get),
            )
            .route(
                "/messages",
                post(mcp_shield::transport::sse::handle_sse_post),
            )
            .with_state(sse_state);
        app = app.merge(sse_router);
        tracing::info!("Legacy SSE transport enabled at /sse and /messages");
    }

    // OAuth discovery endpoints
    if config.auth.enabled {
        let auth_server_url = config
            .auth
            .authorization_server
            .clone()
            .unwrap_or_else(|| format!("http://{}", config.server.bind_addr));

        let bind_addr = config.server.bind_addr;
        let resource_url = format!("http://{}/mcp", bind_addr);

        app = app
            .route(
                "/.well-known/oauth-protected-resource",
                get(move || async move {
                    let prm = mcp_shield::auth::protected_resource_metadata(
                        &resource_url,
                        &[auth_server_url.clone()],
                    );
                    (axum::http::StatusCode::OK, axum::Json(prm))
                }),
            );
        tracing::info!("OAuth PRM endpoint enabled at /.well-known/oauth-protected-resource");
    }

    // Health and metrics endpoints
    app = app
        .route("/healthz", get(health_handler))
        .route("/readyz", get(health_handler));

    app = app.layer(cors).layer(TraceLayer::new_for_http());

    let addr = config.server.bind_addr;
    tracing::info!(addr = %addr, "HTTP server listening");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

/// Check if the config includes an echo upstream.
fn has_echo_upstream(config: &Config) -> bool {
    config.proxy.upstreams.values().any(|u| u.echo_server)
}

/// Build the CORS layer from configuration.
fn build_cors_layer(config: &Config) -> CorsLayer {
    if config.server.cors_origins.is_empty() {
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
            .expose_headers(Any)
    } else {
        let origins: Vec<_> = config
            .server
            .cors_origins
            .iter()
            .filter_map(|o| o.parse().ok())
            .collect();
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods(Any)
            .allow_headers(Any)
            .expose_headers(Any)
    }
}

/// Initialize the tracing subscriber.
fn init_tracing(log_level: &str, json_logging: bool) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level));

    if json_logging {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .with_target(true)
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }
}

/// Health check handler.
async fn health_handler() -> &'static str {
    "ok"
}

/// Wait for a shutdown signal (Ctrl+C).
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Shutdown signal received");
}
