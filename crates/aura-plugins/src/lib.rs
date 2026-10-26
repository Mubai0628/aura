//! Plugins. A plugin is one package the user imports, reviews, trusts, enables, updates and
//! uninstalls as a whole: Skills, prompt commands, MCP servers and Hooks in Aura's own layout,
//! or the parts Aura supports of a Claude Code plugin. Reading a plugin never runs anything;
//! what runs later does so through the MCP and Hook runners, under the plugin's limits.
mod archive;
mod claude;
mod commands;
mod files;
mod manifest;
mod package;
mod servers;
mod settings;
mod views;

pub use archive::{MAX_PLUGIN_ARCHIVE_BYTES, PLUGIN_ARCHIVE, unpack_plugin_archive};
pub use commands::PluginCommand;
pub use files::{MAX_PLUGIN_BYTES, MAX_PLUGIN_FILES, PluginFile, SKIPPED_FOLDERS};
pub use manifest::{
    AURA_MANIFEST, CLAUDE_MANIFEST, DEFAULT_MEMORY_MB, DEFAULT_PROCESSES, MEMORY_MB_RANGE,
    PROCESSES_RANGE, PluginManifest, valid_plugin_name,
};
pub use package::{PluginHook, PluginPackage, PluginPackageError, scan_plugin};
pub use servers::{
    HTTP_SECRET_KEY, PLUGIN_ROOT_VARIABLE, PluginAuth, PluginServer, PluginTransport, server_id,
};
pub use settings::{
    PluginChanges, PluginParts, PluginPendingSetting, PluginSecret, PluginSetting,
    PluginTrustedParts, check_settings,
};
pub use views::*;
