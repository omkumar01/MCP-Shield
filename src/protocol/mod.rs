//! MCP protocol layer.
//!
//! This module provides JSON-RPC 2.0 parsing, MCP message types, and
//! JSON Schema 2020-12 validation for the MCP-Shield gateway.

pub mod jsonrpc;
pub mod message;
pub mod schema;

pub use jsonrpc::{JsonRpcMessage, JsonRpcErrorObj};
pub use message::*;
pub use schema::SchemaValidator;
