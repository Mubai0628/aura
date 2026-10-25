//! The interface settings in 设置 → 环境. A damaged file shows the defaults and an issue;
//! it never changes what a terminal is allowed to run or what the session records.
use crate::memory::current_input_matches;
use crate::{
    ConfigModule, ConfigSnapshot, ConfigValidationError, ModuleConfigDocument, SettingsFacade,
    SettingsFacadeError,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

/// The terminal a new terminal tab opens.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DefaultTerminal {
    #[default]
    PowerShell,
    Cmd,
}
impl DefaultTerminal {
    fn key(self) -> &'static str {
        match self {
            Self::PowerShell => "power_shell",
            Self::Cmd => "cmd",
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiSettings {
    /// The session's runtime readiness and tool execution record, for debugging (default off).
    pub show_debug_information: bool,
    pub default_terminal: DefaultTerminal,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiSettingsSnapshot {
    pub revision: Option<u64>,
    pub settings: UiSettings,
    pub issue: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiSettingsUpdate {
    pub expected_revision: u64,
    pub settings: UiSettings,
}

pub(crate) fn settings_from_document(
    document: &ModuleConfigDocument,
) -> Result<UiSettings, ConfigValidationError> {
    // Fields this version does not use — a theme carried over by the V1 import, or a value another
    // version wrote — are kept as written and ignored; only the two choices here are checked.
    let mut settings = UiSettings::default();
    if let Some(value) = document.values.get("show_debug_information") {
        settings.show_debug_information =
            value.as_bool().ok_or_else(|| ConfigValidationError::InvalidModuleFieldType {
                module: ConfigModule::Ui,
                field: "show_debug_information".into(),
                expected: "boolean",
            })?;
    }
    if let Some(value) = document.values.get("default_terminal") {
        settings.default_terminal = match value.as_str() {
            Some("power_shell") => DefaultTerminal::PowerShell,
            Some("cmd") => DefaultTerminal::Cmd,
            _ => {
                return Err(ConfigValidationError::InvalidModuleFieldType {
                    module: ConfigModule::Ui,
                    field: "default_terminal".into(),
                    expected: "\"power_shell\" or \"cmd\"",
                });
            }
        };
    }
    Ok(settings)
}

impl SettingsFacade {
    pub fn ui_settings(&self) -> UiSettingsSnapshot {
        if self.health(ConfigModule::Ui).state == crate::DomainHealthState::Ready
            && let Ok(snapshot) = self.snapshot(ConfigModule::Ui)
            && let Ok(settings) = settings_from_document(&snapshot.document)
            && current_input_matches(self, ConfigModule::Ui, &snapshot)
        {
            return UiSettingsSnapshot {
                revision: Some(snapshot.document.revision),
                settings,
                issue: None,
            };
        }
        UiSettingsSnapshot {
            revision: None,
            settings: UiSettings::default(),
            issue: Some(
                "界面设置不可用，正在使用默认值。请重试、恢复上一版有效配置或重置此设置域。".into(),
            ),
        }
    }
    pub fn update_ui_settings(
        &self,
        expected_revision: u64,
        settings: &UiSettings,
    ) -> Result<Arc<ConfigSnapshot>, SettingsFacadeError> {
        self.update(ConfigModule::Ui, expected_revision, |values| {
            values.insert(
                "show_debug_information".into(),
                Value::Bool(settings.show_debug_information),
            );
            values.insert(
                "default_terminal".into(),
                Value::String(settings.default_terminal.key().into()),
            );
        })
    }
}
