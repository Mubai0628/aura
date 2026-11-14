use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use aura_data_location::Locations;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigFormat {
    Json,
    Toml,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ConfigModule {
    Aura,
    Models,
    Tools,
    Hooks,
    Mcp,
    Skills,
    Commands,
    Plugins,
    Ui,
    Prompt,
    Security,
    Browser,
    Memory,
    Diagnostics,
    Update,
    Chrome,
    Computer,
    Network,
}

impl ConfigModule {
    pub const ALL: [Self; 18] = [
        Self::Aura,
        Self::Models,
        Self::Tools,
        Self::Hooks,
        Self::Mcp,
        Self::Skills,
        Self::Commands,
        Self::Plugins,
        Self::Ui,
        Self::Prompt,
        Self::Security,
        Self::Browser,
        Self::Memory,
        Self::Diagnostics,
        Self::Update,
        Self::Chrome,
        Self::Computer,
        Self::Network,
    ];

    #[must_use]
    pub const fn format(self) -> ConfigFormat {
        match self {
            Self::Aura
            | Self::Hooks
            | Self::Ui
            | Self::Prompt
            | Self::Security
            | Self::Browser
            | Self::Memory
            | Self::Diagnostics
            | Self::Update
            | Self::Chrome
            | Self::Computer
            | Self::Network => ConfigFormat::Toml,
            Self::Models
            | Self::Tools
            | Self::Mcp
            | Self::Skills
            | Self::Commands
            | Self::Plugins => ConfigFormat::Json,
        }
    }

    #[must_use]
    pub const fn relative_path(self) -> &'static str {
        match self {
            Self::Aura => "aura.toml",
            Self::Models => "models.json",
            Self::Tools => "tools.json",
            Self::Hooks => "hooks.toml",
            Self::Mcp => "mcp.json",
            Self::Skills => "skills.json",
            Self::Commands => "commands.json",
            Self::Plugins => "plugins.json",
            Self::Ui => "settings/ui.toml",
            Self::Prompt => "settings/prompt.toml",
            Self::Security => "settings/security.toml",
            Self::Browser => "settings/browser.toml",
            Self::Memory => "settings/memory.toml",
            Self::Diagnostics => "settings/diagnostics.toml",
            Self::Update => "settings/update.toml",
            Self::Chrome => "settings/chrome.toml",
            Self::Computer => "settings/computer.toml",
            Self::Network => "settings/network.toml",
        }
    }

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Aura => "aura",
            Self::Models => "models",
            Self::Tools => "tools",
            Self::Hooks => "hooks",
            Self::Mcp => "mcp",
            Self::Skills => "skills",
            Self::Commands => "commands",
            Self::Plugins => "plugins",
            Self::Ui => "settings-ui",
            Self::Prompt => "settings-prompt",
            Self::Security => "settings-security",
            Self::Browser => "settings-browser",
            Self::Memory => "settings-memory",
            Self::Diagnostics => "settings-diagnostics",
            Self::Update => "settings-update",
            Self::Chrome => "settings-chrome",
            Self::Computer => "settings-computer",
            Self::Network => "settings-network",
        }
    }

    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self.format() {
            ConfigFormat::Json => "json",
            ConfigFormat::Toml => "toml",
        }
    }
}

/// The files and folders one Core uses, laid out under the locations `aura-data-location` gave it:
/// the settings under `config_root`, everything else under `local_root` (the data folder).
#[derive(Clone, Debug)]
pub struct AuraPaths {
    pub config_root: PathBuf,
    pub local_root: PathBuf,
    pub data_dir: PathBuf,
    pub database_path: PathBuf,
    pub artifact_objects_dir: PathBuf,
    pub quarantine_dir: PathBuf,
    pub config_history_dir: PathBuf,
    pub config_transaction_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub locations: Locations,
}

#[derive(Debug, Error)]
pub enum AuraPathsError {
    #[error("failed to create Aura directory {path}: {source}")]
    CreateDirectory { path: PathBuf, source: std::io::Error },
}

impl AuraPaths {
    /// The layout under an installation's confirmed locations.
    #[must_use]
    pub fn from_locations(locations: Locations) -> Self {
        let mut paths =
            Self::from_roots(locations.settings_root.clone(), locations.data_root.clone());
        paths.locations = locations;
        paths
    }

    /// The layout with the settings in `config_root` and everything else in `local_root`, for
    /// tests and tools that compose a Core over folders of their own.
    #[must_use]
    pub fn from_roots(config_root: PathBuf, local_root: PathBuf) -> Self {
        let data_dir = local_root.join("data");
        let runtime_dir = local_root.join("runtime");
        Self {
            database_path: data_dir.join("aura.db"),
            artifact_objects_dir: data_dir.join("artifact-objects"),
            quarantine_dir: data_dir.join("quarantine"),
            config_history_dir: local_root.join("config-history"),
            config_transaction_dir: runtime_dir.join("config-transactions"),
            logs_dir: local_root.join("logs"),
            locations: Locations::within(config_root.clone(), local_root.clone()),
            config_root,
            local_root,
            data_dir,
            runtime_dir,
        }
    }

    pub fn ensure_layout(&self) -> Result<(), AuraPathsError> {
        let directories = [
            self.config_root.clone(),
            self.config_root.join("settings"),
            self.data_dir.clone(),
            self.artifact_objects_dir.clone(),
            self.quarantine_dir.clone(),
            self.local_root.join("tools"),
            self.local_root.join("skills"),
            self.local_root.join("plugins"),
            self.local_root.join("web"),
            self.local_root.join("worktrees"),
            self.logs_dir.clone(),
            self.runtime_dir.clone(),
            self.config_history_dir.clone(),
            self.config_transaction_dir.clone(),
            self.local_root.join("e2e-runs"),
        ];
        for path in directories {
            fs::create_dir_all(&path)
                .map_err(|source| AuraPathsError::CreateDirectory { path, source })?;
        }
        Ok(())
    }

    #[must_use]
    pub fn config_path(&self, module: ConfigModule) -> PathBuf {
        self.config_root.join(module.relative_path())
    }

    #[must_use]
    pub fn history_path(&self, module: ConfigModule) -> PathBuf {
        self.config_history_dir.join(module.key())
    }

    #[must_use]
    pub fn owns_local_path(&self, path: &Path) -> bool {
        path.is_absolute()
            && path.starts_with(&self.local_root)
            && !path
                .components()
                .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    }
}
