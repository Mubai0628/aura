//! A plugin's background service as Core keeps it: whether it can start (the plugin page turned
//! it on, the plugin can be used and every required setting has a value), the secret settings it
//! is given, its own folders, and the status the host reports for the plugin page.
use super::runtime::{LocatedPlugin, PluginRuntime, ReadyPlugin};
use aura_plugins::{
    PluginOptionKind, PluginOptionValue, PluginPackage, PluginServiceStatus, PluginServiceView,
    PluginSetting, PluginState,
};
use std::collections::BTreeSet;
use std::path::PathBuf;
use zeroize::Zeroizing;

/// Who a secret setting's credential belongs to, in the Credential Manager and its record.
pub(crate) fn secret_owner(plugin: &str, key: &str) -> String {
    format!("plugin-setting:{plugin}/{key}")
}

/// The environment variable a secret setting is given to the service as.
#[must_use]
pub fn secret_variable(key: &str) -> String {
    format!("AURA_SECRET_{}", key.to_ascii_uppercase())
}

/// Why the service of a plugin in `state` cannot start.
fn blocked(
    package: &PluginPackage,
    setting: Option<&PluginSetting>,
    state: PluginState,
) -> Option<String> {
    let why = match state {
        PluginState::Ready => None,
        PluginState::NotTrusted => Some("插件还没有信任"),
        PluginState::ContentChanged => Some("插件的文件在信任后改过，要重新信任"),
        PluginState::Disabled => Some("插件已关闭"),
        PluginState::Invalid | PluginState::Unavailable => Some("插件现在读不出来"),
    };
    if let Some(why) = why {
        return Some(why.to_owned());
    }
    if !package.service_problems.is_empty() {
        return Some(package.service_problems.join("；"));
    }
    let values = setting.map(|setting| &setting.values);
    let missing: Vec<&str> = package
        .manifest
        .declared
        .options
        .iter()
        .filter(|option| option.required)
        .filter(|option| match option.kind {
            PluginOptionKind::Text => !values
                .and_then(|values| values.text.get(&option.key))
                .is_some_and(|text| !text.trim().is_empty()),
            PluginOptionKind::Secret => {
                !values.is_some_and(|values| values.secrets.contains_key(&option.key))
            }
        })
        .map(|option| option.title.as_str())
        .collect();
    (!missing.is_empty()).then(|| format!("缺少设置：{}", missing.join("、")))
}

impl PluginRuntime {
    /// What the host reports for a plugin's service; none once it is no longer running.
    pub fn set_service_status(&self, name: &str, status: Option<PluginServiceStatus>) {
        if let Ok(mut statuses) = self.service_status.lock() {
            match status {
                Some(status) => statuses.insert(name.to_owned(), status),
                None => statuses.remove(name),
            };
        }
    }

    pub(crate) fn service_view(&self, plugin: &LocatedPlugin) -> Option<PluginServiceView> {
        let package = plugin.scanned.as_ref().ok()?;
        package.manifest.declared.service.as_ref()?;
        let on = plugin.setting.as_ref().is_some_and(|setting| setting.remote.service);
        let status = if !on {
            PluginServiceStatus::Off
        } else if let Some(reason) = blocked(package, plugin.setting.as_ref(), plugin.state) {
            PluginServiceStatus::Waiting { reason }
        } else {
            let reported = self.service_status.lock().ok();
            reported
                .and_then(|statuses| statuses.get(&plugin.name).cloned())
                .unwrap_or(PluginServiceStatus::Starting)
        };
        Some(PluginServiceView { on, status })
    }

    /// The plugin's own settings with their values; a secret one only says whether it is set.
    pub(crate) fn option_values(
        package: &PluginPackage,
        setting: Option<&PluginSetting>,
    ) -> Vec<PluginOptionValue> {
        let values = setting.map(|setting| &setting.values);
        package
            .manifest
            .declared
            .options
            .iter()
            .map(|option| PluginOptionValue {
                option: option.clone(),
                text: match option.kind {
                    PluginOptionKind::Text => {
                        values.and_then(|values| values.text.get(&option.key).cloned())
                    }
                    PluginOptionKind::Secret => None,
                },
                set: option.kind == PluginOptionKind::Secret
                    && values.is_some_and(|values| values.secrets.contains_key(&option.key)),
            })
            .collect()
    }

    /// Why a ready plugin's service cannot start yet.
    pub fn service_blocker(&self, ready: &ReadyPlugin) -> Option<String> {
        blocked(&ready.package, Some(&ready.setting), PluginState::Ready)
    }

    /// The secret settings that have values, as the environment variables the service starts
    /// with. Only the host calls this, to start the service.
    pub fn service_secrets(
        &self,
        ready: &ReadyPlugin,
    ) -> Result<Vec<(String, Zeroizing<String>)>, String> {
        let name = &ready.package.manifest.name;
        let options = ready.package.manifest.declared.options.iter();
        let secrets: Vec<_> = options
            .filter(|option| option.kind == PluginOptionKind::Secret)
            .filter_map(|option| Some((option, *ready.setting.values.secrets.get(&option.key)?)))
            .collect();
        if secrets.is_empty() {
            return Ok(Vec::new());
        }
        let mcp = self.mcp.as_ref().ok_or("凭据管理器现在不能用")?;
        secrets
            .into_iter()
            .map(|(option, id)| {
                let (_, value) = mcp
                    .secrets()
                    .value(&secret_owner(name, &option.key), id)
                    .map_err(|reason| format!("保密设置 {} 读不出来：{reason}", option.title))?;
                let text = std::str::from_utf8(value.expose())
                    .map_err(|_| format!("保密设置 {} 不是文字", option.title))?;
                self.remember_log_secret(name, text);
                Ok((secret_variable(&option.key), Zeroizing::new(text.to_owned())))
            })
            .collect()
    }

    /// The service's own folder, and Aura's folder for it.
    pub fn service_folders(&self, name: &str) -> (PathBuf, PathBuf) {
        (self.root().join("data").join(name), self.root().join("run").join(name))
    }

    /// Removes the service folders and the chat state of plugins that are no longer installed.
    /// Skipped while `plugins.json` cannot be read, so a damaged file never costs a plugin its
    /// data.
    pub fn sweep_service_folders(&self) {
        let registry = self.settings().plugin_registry();
        if registry.issue.is_some() {
            return;
        }
        let mut installed: BTreeSet<String> =
            registry.plugins.iter().map(|plugin| plugin.name.clone()).collect();
        if let Ok(entries) = std::fs::read_dir(self.library()) {
            installed.extend(
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.file_name().to_string_lossy().into_owned()),
            );
        }
        for kind in ["data", "run", "state"] {
            let Ok(entries) = std::fs::read_dir(self.root().join(kind)) else { continue };
            for entry in entries.filter_map(Result::ok) {
                let file = entry.file_name().to_string_lossy().into_owned();
                let name = match kind {
                    "state" => file.strip_suffix(".json").unwrap_or(&file),
                    _ => &file,
                };
                if installed.contains(name) {
                    continue;
                }
                if kind == "state" {
                    let _ = std::fs::remove_file(entry.path());
                } else {
                    super::lifecycle::remove_or_log(&entry.path());
                }
            }
        }
    }
}
