//! MCP is aura-extension-service's. Core keeps the route that takes an MCP secret on the bound
//! secret input (`credentials::command`) and the tests that need a started Core; both read MCP's
//! names from here.
pub(crate) use aura_extension_service::mcp::*;
pub(crate) mod credentials;
#[cfg(test)]
pub(crate) mod tests;
