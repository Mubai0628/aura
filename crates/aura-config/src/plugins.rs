//! The installed plugins' choices in `plugins.json` — switched on or off, the content the
//! user trusted, the credentials Aura keeps for their servers' secrets and a newer copy waiting
//! to be trusted. What a plugin brings is read from its folder, never from here.
use crate::{
    ConfigModule, ConfigValidationError, ModuleConfigDocument, SettingsFacade, SettingsFacadeError,
};
use aura_plugins::{PluginSetting, check_settings};
use serde_json::Value;
use std::collections::BTreeMap;

const PLUGINS: &str = "plugins";

/// The choices in force. When `plugins.json` cannot be used, nothing any plugin brings is
/// offered and `issue` says why.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PluginRegistry {
    pub plugins: Vec<PluginSetting>,
    pub revision: u64,
    pub issue: Option<String>,
}

impl PluginRegistry {
    #[must_use]
    pub fn plugin(&self, name: &str) -> Option<&PluginSetting> {
        self.plugins.iter().find(|plugin| plugin.name == name)
    }
}

pub(crate) fn validate_plugins_document(
    document: &ModuleConfigDocument,
) -> Result<(), ConfigValidationError> {
    settings(&document.values).map(|_| ())
}

fn settings(values: &BTreeMap<String, Value>) -> Result<Vec<PluginSetting>, ConfigValidationError> {
    if let Some(field) = values.keys().find(|key| key.as_str() != PLUGINS) {
        return Err(ConfigValidationError::UnknownModuleField {
            module: ConfigModule::Plugins,
            field: field.clone(),
        });
    }
    let Some(items) = values.get(PLUGINS) else { return Ok(Vec::new()) };
    let plugins: Vec<PluginSetting> = serde_json::from_value(items.clone()).map_err(|error| {
        ConfigValidationError::Semantic(format!("plugins.json: plugins 格式不对：{error}"))
    })?;
    check_settings(&plugins)
        .map_err(|reason| ConfigValidationError::Semantic(format!("plugins.json: {reason}")))?;
    Ok(plugins)
}

impl SettingsFacade {
    /// The choices in force. When the domain is not healthy, or the served document is a
    /// last-known-good fallback rather than the file on disk, nothing is offered: a failed edit
    /// may have been the user switching a plugin off or withdrawing trust.
    pub fn plugin_registry(&self) -> PluginRegistry {
        let module = ConfigModule::Plugins;
        let snapshot = match self.snapshot(module) {
            Ok(snapshot)
                if self.health(module).state == crate::DomainHealthState::Ready
                    && crate::memory::current_input_matches(self, module, &snapshot) =>
            {
                snapshot
            }
            _ => {
                return PluginRegistry {
                    issue: Some(
                        "plugins.json 当前无法使用，所有插件带来的内容都不提供。请在设置里重试、恢复上一版有效配置或重置。".into(),
                    ),
                    ..PluginRegistry::default()
                };
            }
        };
        let revision = snapshot.document.revision;
        match settings(&snapshot.document.values) {
            Ok(plugins) => PluginRegistry { plugins, revision, issue: None },
            Err(error) => PluginRegistry {
                plugins: Vec::new(),
                revision,
                issue: Some(format!("{error}；所有插件带来的内容都不提供")),
            },
        }
    }

    /// Replaces the choices at `expected_revision`. A concurrent change is a conflict for the
    /// caller to reread, never a silent overwrite.
    pub fn replace_plugin_registry(
        &self,
        expected_revision: u64,
        plugins: &[PluginSetting],
    ) -> Result<u64, SettingsFacadeError> {
        let items = plugins
            .iter()
            .map(|plugin| serde_json::to_value(plugin).unwrap_or(Value::Null))
            .collect::<Vec<_>>();
        let snapshot = self.update(ConfigModule::Plugins, expected_revision, |values| {
            if items.is_empty() {
                values.remove(PLUGINS);
            } else {
                values.insert(PLUGINS.to_owned(), Value::Array(items));
            }
        })?;
        Ok(snapshot.document.revision)
    }
}
