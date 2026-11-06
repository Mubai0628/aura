//! Plugins. A plugin is one package the user imports, reviews, trusts, enables, updates and
//! uninstalls as a whole: Skills, prompt commands, MCP servers and Hooks in Aura's own layout,
//! or the parts Aura supports of a Claude Code plugin. Reading a plugin never runs anything;
//! what runs later does so through the MCP and Hook runners, under the plugin's limits.
mod archive;
mod channels;
mod claude;
mod commands;
mod declarations;
mod export;
mod files;
mod manifest;
mod package;
mod remote;
mod servers;
mod service_views;
mod settings;
mod theme;
mod validation;
mod views;

pub use archive::{MAX_PLUGIN_ARCHIVE_BYTES, PLUGIN_ARCHIVE, unpack_plugin_archive};
pub use channels::{MAX_CHANNELS, PluginChannel, PluginPlatform, valid_channel_id};
pub use commands::PluginCommand;
pub use declarations::{
    DEFAULT_PANEL_ENTRY, PANEL_FOLDER, PluginCapability, PluginDeclarations, PluginOption,
    PluginOptionKind, PluginPanel, PluginService,
};
pub use export::{PluginExportError, PluginExportSummary, export_plugin};
pub use files::{MAX_PLUGIN_BYTES, MAX_PLUGIN_FILES, PluginFile, SKIPPED_FOLDERS, THEME_FILE};
pub use manifest::{
    AURA_MANIFEST, CLAUDE_MANIFEST, DEFAULT_MEMORY_MB, DEFAULT_PROCESSES, MEMORY_MB_RANGE,
    PROCESSES_RANGE, PluginManifest, valid_plugin_name,
};
pub use package::{PluginHook, PluginPackage, PluginPackageError, scan_plugin};
pub use remote::{
    PLUGIN_ALLOWED_MAX, PLUGIN_EXTERNAL_ID_MAX_CHARS, PLUGIN_EXTERNAL_NAME_MAX_CHARS,
    PLUGIN_TEXT_VALUE_MAX_CHARS, PluginAllowedUser, PluginRemote, PluginScope, PluginValues,
};
pub use servers::{
    HTTP_SECRET_KEY, PLUGIN_ROOT_VARIABLE, PluginAuth, PluginServer, PluginTransport, server_id,
};
pub use service_views::*;
pub use settings::{
    PluginChanges, PluginDevelopment, PluginParts, PluginPendingSetting, PluginSecret,
    PluginSetting, PluginTrustedParts, check_settings,
};
pub use theme::{
    PluginTheme, PluginThemeFonts, PluginThemeScheme, THEME_COLORS, THEME_MAX_FONTS,
    THEME_MAX_RADIUS, THEME_RADII, THEME_TITLE_MAX_CHARS, parse_theme,
};
pub use validation::{PluginValidation, validate_plugin, validation};
pub use views::*;
