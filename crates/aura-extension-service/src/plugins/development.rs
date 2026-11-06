//! Plugins in development: a folder a plugin is being written in, installed where it is. The
//! user adds one on the plugin page; the model adds one with `plugins.dev_install`. Installing
//! never trusts it, switches anything on for it or fills in its settings: the user trusts it on
//! the plugin page, once, and after that its Skills, commands and panel reload by themselves.
use super::lifecycle::replaced;
use super::runtime::{PluginRuntime, problems};
use aura_plugins::{PluginDevelopmentRequest, PluginSetting, PluginView, scan_plugin};
use std::path::Path;

/// An import of a plugin whose name a plugin in development has is refused: the import would
/// otherwise take the folder the user is writing.
pub(super) fn refuse_taken_name(saved: Option<&PluginSetting>) -> anyhow::Result<()> {
    if let Some(development) = saved.and_then(|setting| setting.development.as_ref()) {
        anyhow::bail!(
            "plugins.name_taken: 已有同名的开发中插件，文件夹在 {}；先在插件页卸载它再导入",
            development.folder
        );
    }
    Ok(())
}

impl PluginRuntime {
    /// Installs the plugin in `folder` in development, or returns it as it is when this folder
    /// already is. A name another plugin has is refused.
    pub fn install_development(&self, folder: &Path) -> anyhow::Result<PluginView> {
        let _changing = self.changing.lock().map_err(|_| anyhow::anyhow!("plugins.busy"))?;
        let registry = self.settings().plugin_registry();
        anyhow::ensure!(
            registry.issue.is_none(),
            "plugins.registry_unavailable: {}",
            registry.issue.clone().unwrap_or_default()
        );
        let folder = std::fs::canonicalize(folder)
            .map_err(|_| anyhow::anyhow!("plugins.invalid: 找不到这个文件夹"))?;
        let package = scan_plugin(&folder, &self.hashes)
            .map_err(|error| anyhow::anyhow!("plugins.invalid: {}", problems(&error).join("；")))?;
        let name = package.manifest.name.clone();
        let shown = aura_domain_common::display_path(&folder);
        match registry.plugin(&name).map(|saved| saved.development.as_ref()) {
            Some(Some(development)) if development.folder.eq_ignore_ascii_case(&shown) => {}
            Some(Some(development)) => anyhow::bail!(
                "plugins.name_taken: 已有同名的开发中插件 {name}，文件夹在 {}；先在插件页卸载它，或在清单里换个名字",
                development.folder
            ),
            Some(None) => anyhow::bail!(
                "plugins.name_taken: 已装了同名的插件 {name}；先在插件页卸载它，或在清单里换个名字"
            ),
            None if self.folder(&name).exists() => anyhow::bail!(
                "plugins.name_taken: Aura 的插件文件夹里已有 {name}；先在插件页卸载它，或在清单里换个名字"
            ),
            None => {
                let setting = PluginSetting::in_development(&name, shown);
                self.write_registry(registry.revision, &replaced(&registry, setting))?;
            }
        }
        self.changed();
        let (_, located) = self.locate(&name)?;
        Ok(self.view(&located))
    }

    pub(crate) fn add_development(&self, request: &PluginDevelopmentRequest) -> anyhow::Result<()> {
        let revision = self.settings().plugin_registry().revision;
        anyhow::ensure!(
            revision == request.expected_revision,
            "plugins.revision_changed: 插件设置已被修改，请刷新后再操作"
        );
        self.install_development(Path::new(&request.path)).map(|_| ())
    }
}
