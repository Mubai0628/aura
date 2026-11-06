//! An installed plugin exported as a ZIP file where Main asked the user to save it: the files
//! read just now, checked again as they are copied. Nothing about the plugin changes.
use super::runtime::{PluginRuntime, problems};
use aura_plugins::{PluginExportToRequest, PluginExported, export_plugin};
use std::path::Path;

impl PluginRuntime {
    pub(crate) fn export(&self, request: &PluginExportToRequest) -> anyhow::Result<PluginExported> {
        let destination = Path::new(&request.path);
        anyhow::ensure!(
            destination.is_absolute(),
            "plugins.export_failed: 要保存的位置不是完整路径"
        );
        let (_, located) = self.locate(&request.name)?;
        let package = located.scanned.as_ref().map_err(|error| {
            anyhow::anyhow!(
                "plugins.export_failed: 插件 {} 读不了：{}",
                located.name,
                problems(error).join("；")
            )
        })?;
        let exported = export_plugin(package, destination)
            .map_err(|error| anyhow::anyhow!("plugins.export_failed: {error}"))?;
        Ok(PluginExported {
            completed: true, message: format!("已导出 {} 个文件", exported.files)
        })
    }
}
