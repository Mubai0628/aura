//! MCP servers in Core: what a configuration runs as, one connection per session and
//! server, discovery for the registry, and the settings routes.
pub(crate) mod connection;
pub(crate) mod credentials;
mod http;
pub(crate) mod identity;
mod launch;
mod manage;
pub(crate) mod oauth;
mod runtime;

pub(crate) use manage::McpContext;
pub(crate) use runtime::{McpNetwork, McpRuntime};

#[cfg(test)]
pub(crate) mod tests;
