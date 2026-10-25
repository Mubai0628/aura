//! Settings → 插件: list, preview, import, trust, untrust, switch, discard an update and
//! uninstall. Every change names the `plugins.json` revision it was made against; trust is given
//! only at the fingerprint the user was shown.
use super::runtime::{PluginRuntime, problems};
use aura_plugins::{
    PluginChangeRequest, PluginImportRequest, PluginListRequest, PluginPreview, PluginRequest,
    PluginSetEnabledRequest, PluginTrustRequest,
};
use std::sync::Arc;

pub(crate) struct PluginContext {
    runtime: Arc<PluginRuntime>,
}

impl PluginContext {
    pub(crate) fn new(runtime: Arc<PluginRuntime>) -> Self {
        Self { runtime }
    }

    /// Runs one `agent.plugins.*` Core route.
    pub(crate) fn dispatch(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let result = self.route(method, params);
        if !matches!(method, "agent.plugins.list" | "agent.plugins.preview") {
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
            "agent.plugins.import" => {
                runtime.import(&serde_json::from_value::<PluginImportRequest>(params)?)?;
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
            other => anyhow::bail!("plugins.unknown_route: {other}"),
        }
        Ok(serde_json::to_value(runtime.registry_view())?)
    }

    /// Everything the installed copy (or the waiting update) brings, for review.
    fn preview(&self, request: &PluginRequest) -> anyhow::Result<PluginPreview> {
        let (_, located) = self.runtime.locate(&request.name)?;
        let plugin = self.runtime.view(&located);
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
                fingerprint: Some(package.fingerprint.clone()),
                skills: package.skill_items(),
                commands: package.command_items(),
                servers: package.server_items(),
                hooks: package.hook_items(),
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
                files: Vec::new(),
            },
        })
    }
}
