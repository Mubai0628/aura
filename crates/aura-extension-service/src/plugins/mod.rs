//! Plugins in Core: Aura's plugin folder, the choices in `plugins.json`, the settings routes
//! and, for a ready plugin, what it offers to Skills, commands, MCP and Hooks.
mod changes;
mod chats;
mod configure;
mod development;
mod export;
mod import;
mod lifecycle;
mod logs;
mod manage;
mod runtime;
mod service;
mod theme;

pub use changes::PartKind;
pub use chats::PLUGIN_CHATS_MAX;
pub use manage::PluginContext;
pub use runtime::{PluginRuntime, ReadyPlugin};

#[cfg(test)]
mod tests;
