//! 设置 → 电脑操控. Whether the model may use programs on the user's desktop, the programs every
//! session may use without asking, the programs the user added by hand, and how screenshots and
//! input behave. The settings page writes it; answering a question with “总是允许” adds the
//! program to the always-allowed list.
use crate::memory::current_input_matches;
use crate::{
    ConfigModule, ConfigSnapshot, ConfigValidationError, ModuleConfigDocument, SettingsFacade,
    SettingsFacadeError,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::sync::Arc;

/// The longest edge a screenshot is scaled down to, in pixels: the least, the most and the
/// default.
pub const MIN_SCREENSHOT_EDGE: u32 = 640;
pub const MAX_SCREENSHOT_EDGE: u32 = 3_840;
pub const DEFAULT_SCREENSHOT_EDGE: u32 = 1_568;
/// The most programs either list holds.
pub const MAX_LISTED_APPS: usize = 200;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComputerControlSettings {
    /// Whether the `computer.*` tools are offered (default off).
    pub enabled: bool,
    /// Programs every session reads and acts on without asking, until a session stops one.
    pub always_allowed: Vec<ComputerApp>,
    /// Programs the user added by hand, besides those of the Start menu.
    pub added: Vec<ComputerAddedApp>,
    /// Whether a screenshot paints over what is not an authorized program (default on).
    pub paint_others: bool,
    /// Whether input may use the clipboard, as pasting a long text does (default off).
    pub clipboard: bool,
    /// Whether input may press system chords such as Alt+Tab or the Windows key (default off).
    pub system_keys: bool,
    /// The longest edge of a screenshot the model receives, in pixels.
    pub screenshot_long_edge: u32,
    /// The models that may use the tools, by model ID; empty: every model that takes images.
    pub models: Vec<String>,
    /// The keys that take the desktop over from Aura, besides Escape, in either mode.
    #[serde(default = "default_stop_keys")]
    pub pip_stop_keys: String,
    /// Which desktop a turn uses when its request names none: the user's own, in front, or the
    /// child session in its small window.
    #[serde(default)]
    pub mode: aura_domain_common::ComputerMode,
}

fn default_stop_keys() -> String {
    aura_desktop_domain::pip::PIP_STOP_KEYS.to_owned()
}

impl Default for ComputerControlSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            always_allowed: Vec::new(),
            added: Vec::new(),
            paint_others: true,
            clipboard: false,
            system_keys: false,
            screenshot_long_edge: DEFAULT_SCREENSHOT_EDGE,
            models: Vec::new(),
            pip_stop_keys: default_stop_keys(),
            mode: aura_domain_common::ComputerMode::Foreground,
        }
    }
}

/// A program by its catalog ID, with the name the user saw.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComputerApp {
    pub app_id: String,
    pub name: String,
}

/// A program the user added: its full path, and a name of its own if the user gave one.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComputerAddedApp {
    pub path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComputerControlSettingsSnapshot {
    pub revision: Option<u64>,
    pub settings: ComputerControlSettings,
    pub issue: Option<String>,
}

/// The settings page's save, on the revision it read.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComputerControlSettingsUpdate {
    pub expected_revision: u64,
    pub settings: ComputerControlSettings,
}

fn field_type(field: &str, expected: &'static str) -> ConfigValidationError {
    ConfigValidationError::InvalidModuleFieldType {
        module: ConfigModule::Computer,
        field: field.into(),
        expected,
    }
}

fn flag(
    document: &ModuleConfigDocument,
    key: &str,
    default: bool,
) -> Result<bool, ConfigValidationError> {
    document
        .values
        .get(key)
        .map_or(Ok(default), |value| value.as_bool().ok_or_else(|| field_type(key, "boolean")))
}

/// The tables of a list, each with only the fields it may have.
fn tables<'a>(
    document: &'a ModuleConfigDocument,
    key: &str,
    fields: &[&str],
) -> Result<Vec<&'a Map<String, Value>>, ConfigValidationError> {
    let Some(value) = document.values.get(key) else {
        return Ok(Vec::new());
    };
    let items = value.as_array().ok_or_else(|| field_type(key, "array of tables"))?;
    if items.len() > MAX_LISTED_APPS {
        return Err(field_type(key, "at most 200 programs"));
    }
    items
        .iter()
        .map(|item| {
            item.as_object()
                .filter(|table| table.keys().all(|field| fields.contains(&field.as_str())))
                .ok_or_else(|| field_type(key, "array of tables"))
        })
        .collect()
}

fn text(
    table: &Map<String, Value>,
    field: &str,
    required: bool,
) -> Result<String, ConfigValidationError> {
    match table.get(field) {
        Some(Value::String(value)) if !required || !value.trim().is_empty() => Ok(value.clone()),
        None if !required => Ok(String::new()),
        _ => Err(field_type(field, "string")),
    }
}

pub(crate) fn settings_from_document(
    document: &ModuleConfigDocument,
) -> Result<ComputerControlSettings, ConfigValidationError> {
    let defaults = ComputerControlSettings::default();
    let mut always_allowed = Vec::new();
    for table in tables(document, "always_allowed", &["app_id", "name"])? {
        let app =
            ComputerApp { app_id: text(table, "app_id", true)?, name: text(table, "name", true)? };
        if !always_allowed.iter().any(|known: &ComputerApp| known.app_id == app.app_id) {
            always_allowed.push(app);
        }
    }
    let mut added = Vec::new();
    for table in tables(document, "added", &["path", "name"])? {
        let path = text(table, "path", true)?;
        if !std::path::Path::new(&path).is_absolute() {
            return Err(field_type("added.path", "full path of a program"));
        }
        let app = ComputerAddedApp { path, name: text(table, "name", false)? };
        if !added.iter().any(|known: &ComputerAddedApp| known.path.eq_ignore_ascii_case(&app.path))
        {
            added.push(app);
        }
    }
    let screenshot_long_edge = match document.values.get("screenshot_long_edge") {
        None => defaults.screenshot_long_edge,
        Some(value) => value
            .as_u64()
            .and_then(|edge| u32::try_from(edge).ok())
            .filter(|edge| (MIN_SCREENSHOT_EDGE..=MAX_SCREENSHOT_EDGE).contains(edge))
            .ok_or_else(|| field_type("screenshot_long_edge", "640 to 3840"))?,
    };
    let models = match document.values.get("models") {
        None => Vec::new(),
        Some(value) => value
            .as_array()
            .and_then(|items| items.iter().map(|item| item.as_str().map(str::to_owned)).collect())
            .ok_or_else(|| field_type("models", "array of strings"))?,
    };
    Ok(ComputerControlSettings {
        enabled: flag(document, "enabled", defaults.enabled)?,
        always_allowed,
        added,
        paint_others: flag(document, "paint_others", defaults.paint_others)?,
        clipboard: flag(document, "clipboard", defaults.clipboard)?,
        system_keys: flag(document, "system_keys", defaults.system_keys)?,
        screenshot_long_edge,
        models,
        pip_stop_keys: match document.values.get("pip_stop_keys") {
            None => default_stop_keys(),
            Some(value) => {
                let keys = value.as_str().ok_or_else(|| field_type("pip_stop_keys", "string"))?;
                aura_desktop_domain::pip::stop_chord(keys)
                    .map_err(ConfigValidationError::Semantic)?;
                keys.to_owned()
            }
        },
        mode: match document.values.get("mode") {
            None => defaults.mode,
            Some(value) => serde_json::from_value(value.clone())
                .map_err(|_| field_type("mode", "\"foreground\" or \"child_session\""))?,
        },
    })
}

impl SettingsFacade {
    pub fn computer_settings(&self) -> ComputerControlSettingsSnapshot {
        if self.health(ConfigModule::Computer).state == crate::DomainHealthState::Ready
            && let Ok(snapshot) = self.snapshot(ConfigModule::Computer)
            && let Ok(settings) = settings_from_document(&snapshot.document)
            && current_input_matches(self, ConfigModule::Computer, &snapshot)
        {
            return ComputerControlSettingsSnapshot {
                revision: Some(snapshot.document.revision),
                settings,
                issue: None,
            };
        }
        ComputerControlSettingsSnapshot {
            revision: None,
            settings: ComputerControlSettings::default(),
            issue: Some(
                "电脑操控设置不可用，电脑操控已停用。请重试、恢复上一版有效配置或重置此设置域。"
                    .into(),
            ),
        }
    }

    pub fn update_computer_settings(
        &self,
        expected_revision: u64,
        settings: &ComputerControlSettings,
    ) -> Result<Arc<ConfigSnapshot>, SettingsFacadeError> {
        self.update(ConfigModule::Computer, expected_revision, |values| {
            values.insert("enabled".into(), Value::Bool(settings.enabled));
            values.insert("always_allowed".into(), json!(settings.always_allowed));
            values.insert("added".into(), json!(settings.added));
            values.insert("paint_others".into(), Value::Bool(settings.paint_others));
            values.insert("clipboard".into(), Value::Bool(settings.clipboard));
            values.insert("system_keys".into(), Value::Bool(settings.system_keys));
            // Older versions paused for the user's input; the user's input now takes over.
            values.remove("pause_on_input");
            values.insert("screenshot_long_edge".into(), json!(settings.screenshot_long_edge));
            values.insert("models".into(), json!(settings.models));
            values.insert("pip_stop_keys".into(), json!(settings.pip_stop_keys));
            values.insert("mode".into(), json!(settings.mode));
        })
    }
}
