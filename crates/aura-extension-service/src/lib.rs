//! Extensions in Core: MCP servers, plugins, Skills and Hooks. They hold one another, so they
//! share one crate.
#![forbid(unsafe_code)]

pub mod hooks;
pub mod mcp;
pub mod plugins;
pub mod skills;
