//! MCP is aura-extension-service's. Core keeps the tests that need a started Core; they read
//! MCP's names from here.
pub(crate) use aura_extension_service::mcp::*;
pub(crate) mod credentials;
#[cfg(test)]
pub(crate) mod tests;
