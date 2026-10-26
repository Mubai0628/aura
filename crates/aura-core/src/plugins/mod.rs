//! Plugins in Core: Aura's plugin folder, the choices in `plugins.json`, the settings routes
//! and, for a ready plugin, what it offers to Skills, commands, MCP and Hooks.
mod changes;
mod import;
mod lifecycle;
mod manage;
mod runtime;

pub(crate) use changes::PartKind;
pub(crate) use manage::PluginContext;
pub(crate) use runtime::PluginRuntime;

#[cfg(test)]
mod tests;
