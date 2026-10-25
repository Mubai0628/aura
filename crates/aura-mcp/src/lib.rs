//! MCP: the server configuration, the JSON-RPC wire format, the protocol subset Aura
//! speaks, and how a server's tools become Aura tools.
//!
//! Nothing here starts a process or opens a connection. A server is the user's own program
//! or service: it is reached only after the user trusted its exact configuration, and its
//! tools are external tools whose results the model receives as untrusted content.
mod config;
mod framing;
mod jsonrpc;
mod protocol;
mod schema;
mod sse;
mod tools;
mod views;

pub use config::{
    MAX_SERVERS, McpEnvironmentEntry, McpHeader, McpHttpAuth, McpSecretEnvironment,
    McpServerConfig, McpTransport, ResolvedCommand, config_fingerprint, secret_like,
    validate_server, validate_servers,
};
pub use framing::{FrameError, LineFramer, MAX_MESSAGE_BYTES};
pub use jsonrpc::{
    JsonRpcError, Message, RequestId, RpcError, encode_error, encode_notification, encode_request,
    encode_response, parse_messages,
};
pub use protocol::{
    CallToolResult, ContentBlock, InitializeResult, McpTool, PROTOCOL_VERSIONS, ProtocolError,
    REQUESTED_PROTOCOL_VERSION, ServerInfo, ToolsPage, cancelled_params, initialize_params,
    model_text, parse_call_result, parse_initialize_result, parse_tools_page,
};
pub use schema::input_schema;
pub use sse::{SseEvent, SseParser, SseTooLarge};
pub use tools::{
    MAX_TOOL_DESCRIPTION_CHARS, MAX_TOOL_SCHEMA_BYTES, MAX_TOOLS_PER_SERVER, NormalizedTool,
    ToolCatalog, normalize_tools,
};
pub use views::*;
