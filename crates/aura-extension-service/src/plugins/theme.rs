//! A plugin's theme: given to the window only while the plugin is ready (trusted, switched on and
//! unchanged), and the choice of it taken away with the plugin, so Settings shows Aura's own.
use super::runtime::PluginRuntime;
use aura_config::UiSettings;
use aura_plugins::{PluginState, PluginTheme, PluginThemeRequest};

impl PluginRuntime {
    pub(crate) fn theme(&self, request: &PluginThemeRequest) -> anyhow::Result<PluginTheme> {
        let (_, located) = self.locate(&request.name)?;
        let name = &located.name;
        let package = located
            .scanned
            .as_ref()
            .ok()
            .filter(|_| located.state == PluginState::Ready)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "plugins.theme_unavailable: 插件 {name} 现在不能用，用的是 Aura 自己的主题"
                )
            })?;
        match &package.theme {
            Some(Ok(theme)) => Ok(theme.clone()),
            Some(Err(problems)) => {
                anyhow::bail!(
                    "plugins.theme_invalid: 插件 {name} 的主题有问题：{}",
                    problems.join("；")
                )
            }
            None => anyhow::bail!("plugins.no_theme: 插件 {name} 没有主题"),
        }
    }

    /// The uninstalled plugin's theme is no longer chosen. Should that not be saved, the window
    /// still shows Aura's own: the theme is no longer given.
    pub(crate) fn forget_theme(&self, name: &str) {
        let snapshot = self.settings().ui_settings();
        let Some(revision) = snapshot.revision else { return };
        if snapshot.settings.plugin_theme.as_deref() != Some(name) {
            return;
        }
        let settings = UiSettings { plugin_theme: None, ..snapshot.settings };
        if let Err(error) = self.settings().update_ui_settings(revision, &settings) {
            aura_core_base::append_core_log(
                "plugins.theme_choice_kept",
                serde_json::json!({"plugin": name, "error": error.to_string()}),
            );
        }
    }
}
