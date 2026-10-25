//! Prompt commands: one directory per command (`command.toml` and `template.md`) in the
//! user's Aura configuration or in a workspace's `.aura/commands`. A command is only text
//! the user chooses to send; nothing here executes anything or grants any authority.
mod catalog;
mod definition;
mod manifest;
mod store;
pub use catalog::{add_plugin_commands, disabled_key, merge};
pub use definition::LoadedCommand;
pub use manifest::{
    CommandManifest, MANIFEST_FILE, MAX_MANIFEST_BYTES, MAX_TEMPLATE_BYTES, TEMPLATE_FILE,
    check_template, parse_manifest, render_manifest,
};
pub use store::{CommandStoreError, MAX_COMMANDS, delete_user_command, load, scan, scan_within};

/// The workspace-relative directory that holds a workspace's commands.
pub const WORKSPACE_COMMANDS_DIRECTORY: &str = ".aura/commands";
