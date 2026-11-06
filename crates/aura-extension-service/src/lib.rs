//! Extensions in Core: the Slash command catalog, MCP servers, plugins, Skills and Hooks. They
//! hold one another, so they share one crate.
#![forbid(unsafe_code)]

pub mod commands;
pub mod hooks;
pub mod mcp;
pub mod plugins;
pub mod secret_routes;
pub mod skills;
