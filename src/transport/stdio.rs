//! Stdio transport for MCP-Shield.
//!
//! Reads newline-delimited JSON from stdin and writes JSON lines to stdout.
//! Per the MCP specification, stderr is reserved for diagnostic logging and
//! MUST NOT be used for protocol messages.

use crate::auth::scope::ScopeEnforcer;
use crate::error::McpError;
use crate::gateway::router::McpRouter;
use crate::protocol::jsonrpc::JsonRpcMessage;
use std::io::{BufRead, Write};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader as AsyncBufReader, Stdout};
use tokio::sync::Mutex;

/// The stdio transport handler.
pub struct StdioTransport {
    router: Arc<McpRouter>,
    stdout: Arc<Mutex<Stdout>>,
}

impl StdioTransport {
    /// Create a new stdio transport bound to the given router.
    pub fn new(router: Arc<McpRouter>) -> Self {
        Self {
            router,
            stdout: Arc::new(Mutex::new(tokio::io::stdout())),
        }
    }

    /// Run the stdio transport loop.
    ///
    /// Reads newline-delimited JSON from stdin, processes each message
    /// through the router, and writes responses to stdout. Empty lines
    /// are ignored per the MCP specification.
    pub async fn run(&self, scope_enforcer: ScopeEnforcer) -> Result<(), McpError> {
        tracing::info!("Starting stdio transport");

        let stdin = tokio::io::stdin();
        let mut reader = AsyncBufReader::new(stdin);
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read = reader
                .read_line(&mut line)
                .await
                .map_err(|e| McpError::TransportError(format!("Failed to read stdin: {}", e)))?;

            // EOF — client closed stdin
            if bytes_read == 0 {
                tracing::info!("Stdin closed, shutting down stdio transport");
                break;
            }

            // Skip empty lines (per spec)
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Parse the JSON-RPC message
            let message = match JsonRpcMessage::from_str(trimmed) {
                Ok(msg) => msg,
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to parse stdin message");
                    // Send parse error response
                    let error_response = JsonRpcMessage::parse_error_response();
                    self.write_message(&error_response).await?;
                    continue;
                }
            };

            tracing::debug!(method = ?message.method(), "Processing stdio message");

            // Route the message through the gateway pipeline
            match self
                .router
                .handle_message(message, &scope_enforcer, Some("stdio"))
                .await
            {
                Ok(response) => {
                    self.write_message(&response).await?;
                }
                Err(e) => {
                    // Notifications don't get responses; otherwise send error
                    tracing::debug!(error = %e, "Message handling returned error");
                    if !e.to_string().contains("notification") {
                        let error_response = JsonRpcMessage::Error {
                            id: crate::error::RequestId::Null,
                            error: crate::protocol::jsonrpc::JsonRpcErrorObj::new(
                                e.code(),
                                e.to_string(),
                            ),
                        };
                        self.write_message(&error_response).await?;
                    }
                }
            }
        }

        Ok(())
    }

    /// Write a JSON-RPC message to stdout as a single line.
    async fn write_message(&self, message: &JsonRpcMessage) -> Result<(), McpError> {
        let json = message.to_json_string();
        let mut stdout = self.stdout.lock().await;
        stdout
            .write_all(json.as_bytes())
            .await
            .map_err(|e| McpError::TransportError(format!("Failed to write stdout: {}", e)))?;
        stdout
            .write_all(b"\n")
            .await
            .map_err(|e| McpError::TransportError(format!("Failed to write newline: {}", e)))?;
        stdout
            .flush()
            .await
            .map_err(|e| McpError::TransportError(format!("Failed to flush stdout: {}", e)))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trim_empty_lines() {
        let line = "   \n".trim();
        assert!(line.is_empty());

        let line = "\n".trim();
        assert!(line.is_empty());

        let line = "{\"jsonrpc\":\"2.0\"}\n".trim();
        assert!(!line.is_empty());
    }
}
