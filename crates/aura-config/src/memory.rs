//! Invalid Memory settings disable only automatic maintenance. They never pick a Vault root.
use crate::{
    ConfigModule, ConfigSnapshot, ConfigValidationError, ModuleConfigDocument, SettingsFacade,
    SettingsFacadeError,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemorySettings {
    pub automatic_maintenance: bool,
    #[schemars(range(min = 1, max = 3650))]
    pub event_retention_days: u32,
}
impl Default for MemorySettings {
    fn default() -> Self {
        Self { automatic_maintenance: true, event_retention_days: 30 }
    }
}
impl MemorySettings {
    pub fn validate(&self) -> Result<(), ConfigValidationError> {
        if !(1..=3650).contains(&self.event_retention_days) {
            return Err(ConfigValidationError::Semantic(
                "event_retention_days must be 1..3650".into(),
            ));
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemorySettingsSnapshot {
    pub revision: Option<u64>,
    pub settings: MemorySettings,
    pub issue: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemorySettingsUpdate {
    pub expected_revision: u64,
    pub settings: MemorySettings,
}
pub(crate) fn settings_from_document(
    document: &ModuleConfigDocument,
) -> Result<MemorySettings, ConfigValidationError> {
    for field in document.values.keys() {
        if !matches!(field.as_str(), "automatic_maintenance" | "event_retention_days") {
            return Err(ConfigValidationError::UnknownModuleField {
                module: ConfigModule::Memory,
                field: field.clone(),
            });
        }
    }
    let mut settings = MemorySettings::default();
    if let Some(value) = document.values.get("automatic_maintenance") {
        settings.automatic_maintenance =
            value.as_bool().ok_or_else(|| ConfigValidationError::InvalidModuleFieldType {
                module: ConfigModule::Memory,
                field: "automatic_maintenance".into(),
                expected: "boolean",
            })?;
    }
    if let Some(value) = document.values.get("event_retention_days") {
        settings.event_retention_days = value
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| ConfigValidationError::InvalidModuleFieldType {
                module: ConfigModule::Memory,
                field: "event_retention_days".into(),
                expected: "integer 1..3650",
            })?;
    }
    settings.validate()?;
    Ok(settings)
}
impl SettingsFacade {
    pub fn memory_settings(&self) -> MemorySettingsSnapshot {
        if self.health(ConfigModule::Memory).state == crate::DomainHealthState::Ready
            && let Ok(snapshot) = self.snapshot(ConfigModule::Memory)
            && let Ok(settings) = settings_from_document(&snapshot.document)
            && current_input_matches(self, ConfigModule::Memory, &snapshot)
        {
            return MemorySettingsSnapshot {
                revision: Some(snapshot.document.revision),
                settings,
                issue: None,
            };
        }
        MemorySettingsSnapshot {revision:None,settings:MemorySettings {automatic_maintenance:false,..Default::default()},issue:Some("记忆设置不可用；自动整理已关闭。请重试、恢复上一版有效配置或重置此设置域。记忆文件不会被重置。".into())}
    }
    pub fn update_memory_settings(
        &self,
        expected_revision: u64,
        settings: &MemorySettings,
    ) -> Result<Arc<ConfigSnapshot>, SettingsFacadeError> {
        settings.validate().map_err(|source| SettingsFacadeError::Validation {
            module: ConfigModule::Memory,
            source,
        })?;
        self.update(ConfigModule::Memory, expected_revision, |values| {
            values.insert(
                "automatic_maintenance".into(),
                Value::Bool(settings.automatic_maintenance),
            );
            values.insert("event_retention_days".into(), settings.event_retention_days.into());
        })
    }
}

/// Whether the served snapshot is the file on disk now, not a last-known-good fallback.
pub(crate) fn current_input_matches(
    facade: &SettingsFacade,
    module: ConfigModule,
    snapshot: &ConfigSnapshot,
) -> bool {
    match crate::service::read_config_file(module, &facade.paths().config_path(module)) {
        Ok(bytes) => crate::parse_document(module, &bytes)
            .is_ok_and(|document| document == *snapshot.document),
        Err(SettingsFacadeError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            snapshot.source == crate::ConfigSource::Default
        }
        _ => false,
    }
}
