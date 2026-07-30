//! MCP protocol message types.
//!
//! Strongly-typed representations of all MCP methods: initialize, tools/list,
//! tools/call, resources/list, prompts/list, ping, etc. These are layered on
//! top of the JSON-RPC 2.0 message types from `jsonrpc.rs`.

use crate::error::RequestId;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The MCP protocol version supported by this gateway.
pub const MCP_PROTOCOL_VERSION: &str = "2025-03-26";

// ── Initialize ──────────────────────────────────────────────────────

/// Parameters for the `initialize` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeParams {
    /// The protocol version the client supports.
    pub protocol_version: String,

    /// Capabilities the client provides.
    #[serde(default)]
    pub capabilities: ClientCapabilities,

    /// Client identity information.
    pub client_info: Implementation,
}

/// Result of the `initialize` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    /// The protocol version the server will use.
    pub protocol_version: String,

    /// Capabilities the server provides.
    pub capabilities: ServerCapabilities,

    /// Server identity information.
    pub server_info: Implementation,

    /// Optional instructions for the LLM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

// ── Capabilities ────────────────────────────────────────────────────

/// Client capabilities declared during initialization.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub roots: Option<RootsCapability>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<Value>,
}

/// Client roots capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootsCapability {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Server capabilities declared during initialization.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completions: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub experimental: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<PromptsCapability>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcesCapability>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapability>,
}

/// Server tools capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsCapability {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

/// Server resources capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcesCapability {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscribe: Option<bool>,
}

/// Server prompts capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptsCapability {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

// ── Implementation ───────────────────────────────────────────────────

/// Client or server implementation metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Implementation {
    pub name: String,
    pub version: String,
}

// ── Tools ──────────────────────────────────────────────────────────

/// Parameters for `tools/list`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolsListParams {
    /// Optional cursor for pagination.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,

    /// Metadata with optional progress token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Meta>,
}

/// Result of `tools/list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsListResult {
    pub tools: Vec<Tool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// A tool definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    /// Tool name. Must follow `prefix:name` format in MCP-Shield.
    pub name: String,

    /// Human-readable description of the tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// JSON Schema 2020-12 for input validation.
    pub input_schema: Value,

    /// Optional tool annotations providing hints to clients.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,
}

/// Tool annotations providing hints about tool behavior.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolAnnotations {
    /// Hint that the tool does not modify state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,

    /// Hint that the tool may cause irreversible changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,

    /// Hint that repeated calls produce the same result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,

    /// Hint that the tool interacts with external/unbounded systems.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,

    /// Human-readable title for display.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Parameters for `tools/call`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsCallParams {
    /// The tool name (must follow `prefix:name` format).
    pub name: String,

    /// Tool input arguments, validated against the tool's inputSchema.
    #[serde(default)]
    pub arguments: Value,

    /// Metadata with optional progress token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Meta>,
}

/// Result of `tools/call`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsCallResult {
    /// Array of content blocks.
    pub content: Vec<Content>,

    /// True if the tool call itself returned an error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

// ── Content ─────────────────────────────────────────────────────────

/// A content block in a tool response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Content {
    #[serde(rename = "text")]
    Text {
        text: String,
    },

    #[serde(rename = "image")]
    Image {
        data: String,
        mime_type: String,
    },

    #[serde(rename = "resource")]
    Resource {
        resource: ResourceContents,
    },
}

// ── Resources ────────────────────────────────────────────────────────

/// Parameters for `resources/list`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourcesListParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Meta>,
}

/// Result of `resources/list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcesListResult {
    pub resources: Vec<Resource>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// A resource reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub uri: String,
    pub name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// Resource contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceContents {
    pub uri: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,

    pub text: Option<String>,

    pub blob: Option<String>,
}

// ── Prompts ─────────────────────────────────────────────────────────

/// Parameters for `prompts/list`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PromptsListParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub _meta: Option<Meta>,
}

/// Result of `prompts/list`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptsListResult {
    pub prompts: Vec<Prompt>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// A prompt definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prompt {
    pub name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(default)]
    pub arguments: Vec<PromptArgument>,
}

/// A prompt argument.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptArgument {
    pub name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
}

// ── Ping ────────────────────────────────────────────────────────────

/// Result of `ping`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingResult {}

// ── Logging ─────────────────────────────────────────────────────────

/// A log message notification from server to client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingParams {
    pub level: LoggingLevel,
    pub logger: String,
    pub data: Value,
}

/// Log level.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LoggingLevel {
    Debug,
    Info,
    Notice,
    Warning,
    Error,
    Critical,
    Alert,
    Emergency,
}

// ── Progress ────────────────────────────────────────────────────────

/// Progress notification parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressParams {
    pub progress_token: Value,
    pub progress: f64,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<f64>,
}

// ── Metadata ────────────────────────────────────────────────────────

/// Metadata attached to params.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Meta {
    /// Progress token for tracking long-running operations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress_token: Option<Value>,
}

/// Extract the `_meta` field from a JSON params object.
pub fn extract_meta(params: &Option<Value>) -> Option<Meta> {
    params
        .as_ref()
        .and_then(|p| p.get("_meta"))
        .and_then(|m| serde_json::from_value(m.clone()).ok())
}

/// Extract progress token from metadata.
pub fn extract_progress_token(params: &Option<Value>) -> Option<Value> {
    params
        .as_ref()
        .and_then(|p| p.get("_meta"))
        .and_then(|m| m.get("progressToken").cloned())
}

// ── MCP method constants ────────────────────────────────────────────

pub const METHOD_INITIALIZE: &str = "initialize";
pub const METHOD_INITIALIZED: &str = "notifications/initialized";
pub const METHOD_PING: &str = "ping";
pub const METHOD_TOOLS_LIST: &str = "tools/list";
pub const METHOD_TOOLS_CALL: &str = "tools/call";
pub const METHOD_RESOURCES_LIST: &str = "resources/list";
pub const METHOD_RESOURCES_READ: &str = "resources/read";
pub const METHOD_PROMPTS_LIST: &str = "prompts/list";
pub const METHOD_PROMPTS_GET: &str = "prompts/get";
pub const METHOD_LOGGING: &str = "notifications/message";
pub const METHOD_PROGRESS: &str = "notifications/progress";
pub const METHOD_TOOLS_LIST_CHANGED: &str = "notifications/tools/list_changed";
pub const METHOD_RESOURCES_LIST_CHANGED: &str = "notifications/resources/list_changed";
pub const METHOD_PROMPTS_LIST_CHANGED: &str = "notifications/prompts/list_changed";
pub const METHOD_COMPLETION_COMPLETE: &str = "completion/complete";
pub const METHOD_SAMPLING_CREATE: &str = "sampling/createMessage";
pub const METHOD_SHUTDOWN: &str = "shutdown";
pub const METHOD_EXITED: &str = "notifications/exited";

/// All valid MCP methods.
pub const VALID_METHODS: &[&str] = &[
    METHOD_INITIALIZE,
    METHOD_INITIALIZED,
    METHOD_PING,
    METHOD_TOOLS_LIST,
    METHOD_TOOLS_CALL,
    METHOD_RESOURCES_LIST,
    METHOD_RESOURCES_READ,
    METHOD_PROMPTS_LIST,
    METHOD_PROMPTS_GET,
    METHOD_LOGGING,
    METHOD_PROGRESS,
    METHOD_TOOLS_LIST_CHANGED,
    METHOD_RESOURCES_LIST_CHANGED,
    METHOD_PROMPTS_LIST_CHANGED,
    METHOD_COMPLETION_COMPLETE,
    METHOD_SAMPLING_CREATE,
    METHOD_SHUTDOWN,
    METHOD_EXITED,
];

/// Check if a method is a known MCP method.
pub fn is_valid_method(method: &str) -> bool {
    VALID_METHODS.contains(&method)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_initialize_params() {
        let raw = r#"{
            "protocolVersion": "2025-03-26",
            "capabilities": {"roots": {"listChanged": true}},
            "clientInfo": {"name": "test-client", "version": "1.0.0"}
        }"#;
        let params: InitializeParams = serde_json::from_str(raw).unwrap();
        assert_eq!(params.protocol_version, MCP_PROTOCOL_VERSION);
        assert_eq!(params.client_info.name, "test-client");
        assert!(params.capabilities.roots.is_some());
    }

    #[test]
    fn test_deserialize_tools_call_params() {
        let raw = r#"{
            "name": "com.example.echo",
            "arguments": {"message": "hello"},
            "_meta": {"progressToken": "pt-123"}
        }"#;
        let params: ToolsCallParams = serde_json::from_str(raw).unwrap();
        assert_eq!(params.name, "com.example.echo");
        assert_eq!(params.arguments["message"], "hello");
        assert!(params._meta.is_some());
    }

    #[test]
    fn test_deserialize_tool_with_annotations() {
        let raw = r#"{
            "name": "com.example.delete",
            "description": "Deletes a record",
            "inputSchema": {"type": "object", "properties": {"id": {"type": "integer"}}},
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": true,
                "idempotentHint": true,
                "openWorldHint": false,
                "title": "Delete Record"
            }
        }"#;
        let tool: Tool = serde_json::from_str(raw).unwrap();
        assert_eq!(tool.name, "com.example.delete");
        assert!(tool.annotations.is_some());
        let ann = tool.annotations.unwrap();
        assert_eq!(ann.read_only_hint, Some(false));
        assert_eq!(ann.destructive_hint, Some(true));
    }

    #[test]
    fn test_deserialize_content_types() {
        let text = r#"{"type": "text", "text": "Hello"}"#;
        let content: Content = serde_json::from_str(text).unwrap();
        match content {
            Content::Text { text } => assert_eq!(text, "Hello"),
            _ => panic!("Expected Text content"),
        }

        let image = r#"{"type": "image", "data": "base64...", "mime_type": "image/png"}"#;
        let content: Content = serde_json::from_str(image).unwrap();
        match content {
            Content::Image { mime_type, .. } => assert_eq!(mime_type, "image/png"),
            _ => panic!("Expected Image content"),
        }
    }

    #[test]
    fn test_extract_progress_token() {
        let params = Some(json!({"_meta": {"progressToken": "pt-abc"}}));
        let token = extract_progress_token(&params);
        assert_eq!(token, Some(json!("pt-abc")));

        let no_meta = Some(json!({"key": "value"}));
        assert!(extract_progress_token(&no_meta).is_none());
    }

    #[test]
    fn test_valid_methods() {
        assert!(is_valid_method("initialize"));
        assert!(is_valid_method("tools/call"));
        assert!(is_valid_method("notifications/initialized"));
        assert!(!is_valid_method("unknown/method"));
    }

    #[test]
    fn test_serialize_tools_list_result() {
        let result = ToolsListResult {
            tools: vec![Tool {
                name: "com.example.echo".to_string(),
                description: Some("Echo tool".to_string()),
                input_schema: json!({"type": "object"}),
                annotations: None,
            }],
            next_cursor: None,
        };
        let serialized = serde_json::to_string(&result).unwrap();
        let deserialized: ToolsListResult = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.tools.len(), 1);
        assert_eq!(deserialized.tools[0].name, "com.example.echo");
    }
}
