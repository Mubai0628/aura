//! 设置 → 网络与浏览器 → Chrome. Whether Aura's Chrome extension may connect, and which extension
//! is paired with this installation. The pairing secret is in the Windows Credential Manager; this
//! file only names its entry. Core writes the pairing; the page only turns the switch.
use crate::memory::current_input_matches;
use crate::{
    ConfigModule, ConfigSnapshot, ConfigValidationError, ModuleConfigDocument, SettingsFacade,
    SettingsFacadeError,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChromeControlSettings {
    /// Whether Chrome may start Aura's relay for the extension (default off).
    pub enabled: bool,
    /// The extension paired with this installation, if one is.
    pub paired: Option<ChromePairing>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChromePairing {
    pub extension_id: String,
    /// The Credential Manager entry holding the pairing secret.
    pub credential_id: String,
    pub paired_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChromeControlSettingsSnapshot {
    pub revision: Option<u64>,
    pub settings: ChromeControlSettings,
    pub issue: Option<String>,
}

fn field_type(field: &str, expected: &'static str) -> ConfigValidationError {
    ConfigValidationError::InvalidModuleFieldType {
        module: ConfigModule::Chrome,
        field: field.into(),
        expected,
    }
}

pub(crate) fn settings_from_document(
    document: &ModuleConfigDocument,
) -> Result<ChromeControlSettings, ConfigValidationError> {
    let mut settings = ChromeControlSettings::default();
    if let Some(enabled) = document.values.get("enabled") {
        settings.enabled = enabled.as_bool().ok_or_else(|| field_type("enabled", "boolean"))?;
    }
    if let Some(paired) = document.values.get("paired") {
        let table = paired.as_object().ok_or_else(|| field_type("paired", "table"))?;
        let text = |key: &str| {
            table
                .get(key)
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| field_type(&format!("paired.{key}"), "string"))
        };
        settings.paired = Some(ChromePairing {
            extension_id: text("extension_id")?,
            credential_id: text("credential_id")?,
            paired_at: text("paired_at")?,
        });
    }
    Ok(settings)
}

impl SettingsFacade {
    pub fn chrome_settings(&self) -> ChromeControlSettingsSnapshot {
        if self.health(ConfigModule::Chrome).state == crate::DomainHealthState::Ready
            && let Ok(snapshot) = self.snapshot(ConfigModule::Chrome)
            && let Ok(settings) = settings_from_document(&snapshot.document)
            && current_input_matches(self, ConfigModule::Chrome, &snapshot)
        {
            return ChromeControlSettingsSnapshot {
                revision: Some(snapshot.document.revision),
                settings,
                issue: None,
            };
        }
        ChromeControlSettingsSnapshot {
            revision: None,
            settings: ChromeControlSettings::default(),
            issue: Some(
                "Chrome 设置不可用，Chrome 控制已停用。请重试、恢复上一版有效配置或重置此设置域。"
                    .into(),
            ),
        }
    }

    pub fn update_chrome_settings(
        &self,
        expected_revision: u64,
        settings: &ChromeControlSettings,
    ) -> Result<Arc<ConfigSnapshot>, SettingsFacadeError> {
        self.update(ConfigModule::Chrome, expected_revision, |values| {
            values.insert("enabled".into(), Value::Bool(settings.enabled));
            match &settings.paired {
                Some(paired) => {
                    values.insert(
                        "paired".into(),
                        serde_json::json!({
                            "extension_id": paired.extension_id,
                            "credential_id": paired.credential_id,
                            "paired_at": paired.paired_at,
                        }),
                    );
                }
                None => {
                    values.remove("paired");
                }
            }
        })
    }
}
