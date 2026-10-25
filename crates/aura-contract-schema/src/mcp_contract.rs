//! MCP: Settings → MCP.
use aura_mcp::*;
pub(super) fn register(generator: &mut schemars::SchemaGenerator) {
    generator.subschema_for::<McpServerConfig>();
    generator.subschema_for::<McpServerState>();
    generator.subschema_for::<McpServerView>();
    generator.subschema_for::<McpRegistryView>();
    generator.subschema_for::<McpListRequest>();
    generator.subschema_for::<McpPreviewRequest>();
    generator.subschema_for::<McpPreview>();
    generator.subschema_for::<McpSaveRequest>();
    generator.subschema_for::<McpTrustRequest>();
    generator.subschema_for::<McpServerRequest>();
    generator.subschema_for::<McpSetEnabledRequest>();
    generator.subschema_for::<McpRefreshRequest>();
    generator.subschema_for::<McpDescribeRequest>();
    generator.subschema_for::<McpToolView>();
    generator.subschema_for::<McpServerDetail>();
    // Setting a server's secret (the value travels on the secret input only).
    generator.subschema_for::<McpSecretSetRequest>();
    generator.subschema_for::<McpSecretSetResult>();
    // OAuth sign-in (Main opens the browser; Core keeps the tokens).
    generator.subschema_for::<McpSignInView>();
    generator.subschema_for::<McpSignInRequest>();
    generator.subschema_for::<McpSignInBegun>();
    generator.subschema_for::<McpSignOutRequest>();
}
