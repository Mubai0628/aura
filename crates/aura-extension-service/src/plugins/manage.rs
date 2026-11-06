//! Settings → 插件: list, preview, import, trust, untrust, switch, discard an update,
//! uninstall, export and the background service's settings; and a ready plugin's theme for the
//! window. Every change names the `plugins.json` revision it was made against; trust is given
//! only at the fingerprint the user was shown.
use super::runtime::{PluginRuntime, problems, trusted_at};
use aura_plugins::{
    PluginChangeRequest, PluginConfigureRequest, PluginDevelopmentRequest, PluginExportToRequest,
    PluginImportRequest, PluginListRequest, PluginPreview, PluginRequest, PluginSetEnabledRequest,
    PluginThemeRequest, PluginTrustRequest,
};
use std::sync::Arc;

pub struct PluginContext {
    runtime: Arc<PluginRuntime>,
}

impl PluginContext {
    pub fn new(runtime: Arc<PluginRuntime>) -> Self {
        Self { runtime }
    }

    /// Runs one `agent.plugins.*` Core route.
    pub fn dispatch(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let result = self.route(method, params);
        if !matches!(
            method,
            "agent.plugins.list"
                | "agent.plugins.preview"
                | "agent.plugins.theme"
                | "agent.plugins.export"
        ) {
            // What a ready plugin offers is read again on the next use.
            self.runtime.changed();
        }
        result
    }

    fn route(&self, method: &str, params: serde_json::Value) -> anyhow::Result<serde_json::Value> {
        let runtime = &self.runtime;
        match method {
            "agent.plugins.list" => {
                let _: PluginListRequest = serde_json::from_value(params)?;
            }
            "agent.plugins.preview" => {
                return Ok(serde_json::to_value(self.preview(&serde_json::from_value(params)?)?)?);
            }
            "agent.plugins.theme" => {
                let request: PluginThemeRequest = serde_json::from_value(params)?;
                return Ok(serde_json::to_value(runtime.theme(&request)?)?);
            }
            "agent.plugins.export" => {
                let request: PluginExportToRequest = serde_json::from_value(params)?;
                return Ok(serde_json::to_value(runtime.export(&request)?)?);
            }
            "agent.plugins.import" => {
                runtime.import(&serde_json::from_value::<PluginImportRequest>(params)?)?;
            }
            "agent.plugins.add_development" => {
                runtime.add_development(&serde_json::from_value::<PluginDevelopmentRequest>(
                    params,
                )?)?;
            }
            "agent.plugins.trust" => {
                runtime.trust(&serde_json::from_value::<PluginTrustRequest>(params)?)?;
            }
            "agent.plugins.untrust" => {
                runtime.untrust(&serde_json::from_value::<PluginChangeRequest>(params)?)?;
            }
            "agent.plugins.set_enabled" => {
                runtime.set_enabled(&serde_json::from_value::<PluginSetEnabledRequest>(params)?)?;
            }
            "agent.plugins.discard_update" => {
                runtime.discard_update(&serde_json::from_value::<PluginChangeRequest>(params)?)?;
            }
            "agent.plugins.uninstall" => {
                runtime.uninstall(&serde_json::from_value::<PluginChangeRequest>(params)?)?;
            }
            "agent.plugins.configure" => {
                runtime.configure(&serde_json::from_value::<PluginConfigureRequest>(params)?)?;
            }
            other => anyhow::bail!("plugins.unknown_route: {other}"),
        }
        Ok(serde_json::to_value(runtime.registry_view())?)
    }

    /// Everything the installed copy (or the waiting update) brings, for review.
    fn preview(&self, request: &PluginRequest) -> anyhow::Result<PluginPreview> {
        let (_, located) = self.runtime.locate(&request.name)?;
        let plugin = self.runtime.view(&located);
        let setting = located.setting.clone();
        let scanned = if request.pending {
            let folder = self.runtime.pending_folder(&request.name);
            anyhow::ensure!(folder.is_dir(), "plugins.no_update: 没有等待信任的新版本");
            self.runtime.scan(&folder)
        } else {
            located.scanned
        };
        Ok(match scanned {
            Ok(package) => PluginPreview {
                plugin,
                pending: request.pending,
                fingerprint: Some(trusted_at(&package, setting.as_ref()).to_owned()),
                skills: package.skill_items(),
                commands: package.command_items(),
                servers: package.server_items(),
                hooks: package.hook_items(),
                capabilities: package.manifest.declared.capabilities.clone(),
                panel: package.panel_item(),
                service: package.service_item(),
                options: package.manifest.declared.options.clone(),
                theme: package.theme_item(),
                channels: package.manifest.declared.channels.clone(),
                files: package.files,
            },
            Err(error) if request.pending => {
                anyhow::bail!("plugins.invalid: {}", problems(&error).join("；"))
            }
            Err(_) => PluginPreview {
                plugin,
                pending: false,
                fingerprint: None,
                skills: Vec::new(),
                commands: Vec::new(),
                servers: Vec::new(),
                hooks: Vec::new(),
                capabilities: Vec::new(),
                panel: None,
                service: None,
                options: Vec::new(),
                theme: None,
                channels: Vec::new(),
                files: Vec::new(),
            },
        })
    }
}
