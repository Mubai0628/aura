//! The Workspace Files settings in `tools.json`: the catalog, read and watcher bounds the
//! workspace runtime and the file tools are sized by, read and checked as one document.
use anyhow::{Context, Result};
use aura_config::{
    ConfigModule, ConfigValidationError, ModuleConfigDocument, ModuleConfigValidator,
    SettingsFacade,
};
use aura_core_base::config_degradation;
use aura_fs_tools::FsToolPolicy;
use aura_workspace::{CatalogOptions, WorkspaceRuntimeOptions};
use aura_workspace_domain::LargeFileReadMode;
use serde::Deserialize;

const WORKSPACE_FILES_CONFIG_KEY: &str = "workspace_files";

#[derive(Clone, Copy, Debug, Default)]
pub struct P03ToolsConfigValidator;

impl ModuleConfigValidator for P03ToolsConfigValidator {
    fn validate(
        &self,
        module: ConfigModule,
        document: &ModuleConfigDocument,
    ) -> Result<(), ConfigValidationError> {
        if module != ConfigModule::Tools {
            return Ok(());
        }
        WorkspaceFilesConfig::from_document(document)
            .map(|_| ())
            .map_err(|error| ConfigValidationError::Semantic(error.to_string()))
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct WorkspaceFilesConfig {
    pub(super) catalog: CatalogConfig,
    pub(super) limits: LimitsConfig,
    pub(super) read: ReadConfig,
    pub(super) watcher: WatcherConfig,
}

impl WorkspaceFilesConfig {
    /// Reads the Workspace Files settings, or reports the real configuration
    /// fault with the domain that caused it.
    ///
    /// The settings are a genuine dependency of the workspace runtime, so
    /// this refuses rather than inventing bounds. What changed is that the
    /// refusal names the domain and the stable fault code, which is what lets the
    /// caller degrade **one capability** instead of the whole runtime.
    pub(super) fn from_settings(settings: &SettingsFacade) -> Result<Self> {
        let snapshot = settings.snapshot(ConfigModule::Tools).map_err(|error| {
            anyhow::anyhow!(
                "core.tool_policy.config_unavailable: 工具设置（{} 配置）读不出来，工作区与文件工具暂时不能用；请到设置里查看恢复选项。（{}：{error}）",
                error.module().map_or("unknown", ConfigModule::key),
                config_degradation::config_fault_code(&error),
            )
        })?;
        Self::from_document(&snapshot.document)
    }

    pub(super) fn from_document(document: &ModuleConfigDocument) -> Result<Self> {
        let config = document.values.get(WORKSPACE_FILES_CONFIG_KEY).map_or_else(
            || Ok(Self::default()),
            |value| serde_json::from_value(value.clone()).context("config.workspace_files_invalid: 工具设置（tools.json）里的 workspace_files 有不认识的项或值的格式不对，这份设置不能用；请改正后再保存，或到设置里查看恢复选项。"),
        )?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.catalog.max_partition_files > 0,
            "config.workspace_files_invalid: 工具设置里 workspace_files 的 catalog 中 max_partition_files 必须大于 0，这份设置不能用；请改正后再保存，或到设置里查看恢复选项。"
        );
        anyhow::ensure!(
            self.catalog.max_partition_text_bytes > 0,
            "config.workspace_files_invalid: 工具设置里 workspace_files 的 catalog 中 max_partition_text_bytes 必须大于 0，这份设置不能用；请改正后再保存，或到设置里查看恢复选项。"
        );
        anyhow::ensure!(
            self.catalog.max_index_file_bytes > 0,
            "config.workspace_files_invalid: 工具设置里 workspace_files 的 catalog 中 max_index_file_bytes 必须大于 0，这份设置不能用；请改正后再保存，或到设置里查看恢复选项。"
        );
        anyhow::ensure!(
            self.watcher.coalesce_milliseconds > 0,
            "config.workspace_files_invalid: 工具设置里 workspace_files 的 watcher 中 coalesce_milliseconds 必须大于 0，这份设置不能用；请改正后再保存，或到设置里查看恢复选项。"
        );
        anyhow::ensure!(
            self.watcher.poll_interval_milliseconds > 0,
            "config.workspace_files_invalid: 工具设置里 workspace_files 的 watcher 中 poll_interval_milliseconds 必须大于 0，这份设置不能用；请改正后再保存，或到设置里查看恢复选项。"
        );
        anyhow::ensure!(
            self.watcher.rescan_attempts > 0,
            "config.workspace_files_invalid: 工具设置里 workspace_files 的 watcher 中 rescan_attempts 必须大于 0，这份设置不能用；请改正后再保存，或到设置里查看恢复选项。"
        );
        FsToolPolicy {
            large_file_read_mode: self.read.large_file_read_mode,
            full_text_read_limit_bytes: self.read.full_text_read_limit_bytes,
            small_range_line_limit: self.read.small_range_line_limit,
            small_range_character_limit: self.read.small_range_character_limit,
            default_binary_range_bytes: self.read.default_binary_range_bytes,
            max_binary_range_bytes: self.read.max_binary_range_bytes,
            max_changeset_operations: self.limits.max_changeset_operations,
            max_search_results: self.limits.max_search_results,
            mutation_wait: std::time::Duration::from_secs(5),
        }
        .validate()
        .map_err(anyhow::Error::msg)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct CatalogConfig {
    pub(super) max_partition_files: u64,
    pub(super) max_partition_text_bytes: u64,
    pub(super) max_index_file_bytes: u64,
}

impl Default for CatalogConfig {
    fn default() -> Self {
        let defaults = CatalogOptions::default();
        Self {
            max_partition_files: defaults.max_partition_files,
            max_partition_text_bytes: defaults.max_partition_text_bytes,
            max_index_file_bytes: defaults.max_index_file_bytes,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct ReadConfig {
    pub(super) large_file_read_mode: LargeFileReadMode,
    pub(super) full_text_read_limit_bytes: u64,
    pub(super) small_range_line_limit: u64,
    pub(super) small_range_character_limit: u64,
    pub(super) default_binary_range_bytes: u32,
    pub(super) max_binary_range_bytes: u32,
}

impl Default for ReadConfig {
    fn default() -> Self {
        let defaults = FsToolPolicy::default();
        Self {
            large_file_read_mode: defaults.large_file_read_mode,
            full_text_read_limit_bytes: defaults.full_text_read_limit_bytes,
            small_range_line_limit: defaults.small_range_line_limit,
            small_range_character_limit: defaults.small_range_character_limit,
            default_binary_range_bytes: defaults.default_binary_range_bytes,
            max_binary_range_bytes: defaults.max_binary_range_bytes,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct LimitsConfig {
    pub(super) max_changeset_operations: usize,
    pub(super) max_search_results: usize,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        let defaults = FsToolPolicy::default();
        Self {
            max_changeset_operations: defaults.max_changeset_operations,
            max_search_results: defaults.max_search_results,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct WatcherConfig {
    pub(super) coalesce_milliseconds: u64,
    pub(super) poll_interval_milliseconds: u64,
    pub(super) rescan_attempts: u32,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        let defaults = WorkspaceRuntimeOptions::default();
        Self {
            coalesce_milliseconds: u64::try_from(defaults.watcher_coalesce.as_millis())
                .expect("default coalesce fits u64"),
            poll_interval_milliseconds: u64::try_from(defaults.watcher_poll_interval.as_millis())
                .expect("default poll interval fits u64"),
            rescan_attempts: defaults.watcher_rescan_attempts,
        }
    }
}

#[cfg(test)]
mod tests {
    use aura_config::ModuleConfigDocument;
    use serde_json::json;

    use super::WorkspaceFilesConfig;

    #[test]
    fn confirmed_p03_defaults_are_stable() {
        let config = WorkspaceFilesConfig::from_document(&ModuleConfigDocument::empty())
            .expect("default P03 tools config");
        assert_eq!(config.read.full_text_read_limit_bytes, 2 * 1024 * 1024);
        assert_eq!(config.catalog.max_index_file_bytes, 1024 * 1024);
        assert_eq!(config.read.small_range_line_limit, 500);
        assert_eq!(config.read.small_range_character_limit, 65_536);
        assert_eq!(config.limits.max_changeset_operations, 64);
        assert_eq!(config.limits.max_search_results, 100);
        assert_eq!(config.watcher.coalesce_milliseconds, 250);
        assert_eq!(config.watcher.poll_interval_milliseconds, 5_000);
        assert_eq!(config.watcher.rescan_attempts, 3);
    }

    #[test]
    fn tools_json_overrides_are_typed_and_unknown_fields_are_rejected() {
        let mut document = ModuleConfigDocument::empty();
        document.values.insert(
            "workspace_files".to_owned(),
            json!({
                "read": { "full_text_read_limit_bytes": 4 * 1024 * 1024 },
                "limits": { "max_search_results": 75 }
            }),
        );
        let config = WorkspaceFilesConfig::from_document(&document).expect("valid override");
        assert_eq!(config.read.full_text_read_limit_bytes, 4 * 1024 * 1024);
        assert_eq!(config.limits.max_search_results, 75);
        assert_eq!(config.watcher.rescan_attempts, 3);

        document.values.insert(
            "workspace_files".to_owned(),
            json!({ "read": { "private_unreviewed_limit": 1 } }),
        );
        assert!(WorkspaceFilesConfig::from_document(&document).is_err());
    }

    #[test]
    fn invalid_zero_watcher_retry_count_is_rejected() {
        let mut document = ModuleConfigDocument::empty();
        document
            .values
            .insert("workspace_files".to_owned(), json!({ "watcher": { "rescan_attempts": 0 } }));
        assert!(WorkspaceFilesConfig::from_document(&document).is_err());
    }
}
