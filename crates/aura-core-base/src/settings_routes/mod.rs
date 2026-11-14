//! Settings routes that start nothing a model can reach: 设置 → 环境 (`ui.settings.*`),
//! → 网络与浏览器 (`browser.settings.*`) and → 系统 → 存储 (`storage.*`).
use std::path::Path;

use aura_config::SettingsFacade;
use aura_domain_common::CommandInvocationId;
use aura_workspace_domain::WorkspaceSessionProjection;
use serde_json::Value;

pub mod browser;
pub mod storage;
pub mod ui;

/// Whether `method` is one of these routes.
#[must_use]
pub fn serves(method: &str) -> bool {
    ["ui.settings.", "browser.settings.", "storage."]
        .iter()
        .any(|prefix| method.starts_with(prefix))
}

/// What the routes read besides the request: the settings, the release's manifest (which says
/// whether this is an installation) and the Workspaces, whose folders the data stays out of.
pub struct Served<'a> {
    pub settings: &'a SettingsFacade,
    pub manifest: &'a Path,
    pub workspaces: &'a dyn Fn() -> anyhow::Result<WorkspaceSessionProjection>,
}

pub fn dispatch(
    served: &Served<'_>,
    method: &str,
    params: Value,
    command: Option<CommandInvocationId>,
) -> anyhow::Result<Value> {
    if method.starts_with("ui.settings.") {
        ui::dispatch(served.settings, method, params, command)
    } else if method.starts_with("browser.settings.") {
        browser::dispatch(served.settings, method, params, command)
    } else {
        storage::dispatch(served.manifest, served.workspaces, method, params, command)
    }
}
