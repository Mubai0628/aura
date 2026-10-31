//! MCP servers in Core: what a configuration runs as, one connection per session and
//! server, discovery for the registry, and the settings routes.
pub mod connection;
pub mod credentials;
mod http;
pub mod identity;
mod launch;
mod manage;
pub(crate) mod oauth;
mod runtime;

pub use manage::McpContext;
pub use runtime::{McpNetwork, McpRuntime};
