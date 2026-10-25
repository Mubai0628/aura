use std::{
    env, fs,
    path::{Component, Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use aura_runtime_contract::BundleNamespace;

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
}

impl ConfigModule {
    pub const ALL: [Self; 15] = [
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
            | Self::Update => ConfigFormat::Toml,
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
}

#[derive(Debug, Error)]
pub enum AuraPathsError {
    #[error("required Windows environment variable {0} is missing")]
    MissingEnvironment(&'static str),
    #[error("failed to create Aura directory {path}: {source}")]
    CreateDirectory { path: PathBuf, source: std::io::Error },
}

impl AuraPaths {
    pub fn from_environment() -> Result<Self, AuraPathsError> {
        let namespace: BundleNamespace =
            "aura.desktop.production".parse().expect("the frozen production namespace is valid");
        Self::from_environment_for_namespace(&namespace)
    }

    pub fn from_environment_for_namespace(
        namespace: &BundleNamespace,
    ) -> Result<Self, AuraPathsError> {
        let app_data =
            env::var_os("APPDATA").ok_or(AuraPathsError::MissingEnvironment("APPDATA"))?;
        let local_app_data = env::var_os("LOCALAPPDATA")
            .ok_or(AuraPathsError::MissingEnvironment("LOCALAPPDATA"))?;
        let namespace_partition = namespace.object_name_hash();
        Ok(Self::from_roots(
            PathBuf::from(app_data).join("Aura").join("namespaces").join(&namespace_partition),
            PathBuf::from(local_app_data).join("Aura").join("namespaces").join(namespace_partition),
        ))
    }

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
