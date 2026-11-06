//! `plugins.*` model tools, for writing a plugin with the user. The folder is one of this
//! workspace's, found where it really is; installing records it as a plugin in development and
//! nothing else: trust, what the plugin asks for and its settings stay the user's, on the plugin
//! page. Folders are read before the store transaction; only the answer is recorded inside it.
use super::*;
use aura_agent_domain::{
    PluginsDevInstallInput, PluginsDevInstallResult, PluginsLogsInput, PluginsLogsResult,
    PluginsValidateInput, PluginsValidateResult,
};
use aura_extension_service::plugins::PluginRuntime;
use serde_json::Value;
use std::path::PathBuf;

const DEFAULT_LOGS: usize = 50;
const MAX_LOGS: usize = 200;

// The model reads one of these fixed sentences when a call is refused.
const ARGUMENTS: &str = "plugins.arguments_invalid: 参数不对，请按工具说明填写";
const OUTSIDE: &str =
    "plugins.path_invalid: path 必须是这个工作区里的文件夹，相对工作区根目录写，不能是链接";
const NOT_FOUND: &str = "plugins.not_found: 没有这个名字的插件；名字以插件清单里的为准";
const NOT_NOW: &str = "plugins.unavailable_now: 暂时用不了插件，请稍后再试";

pub(in crate::agent_execution) fn is_plugin_tool(action_kind: &str) -> bool {
    action_kind.starts_with("plugins.")
}

fn refusal(error: &anyhow::Error) -> &'static str {
    let text = error.to_string();
    [ARGUMENTS, OUTSIDE, NOT_FOUND].into_iter().find(|known| *known == text).unwrap_or(NOT_NOW)
}

fn input<T: serde::de::DeserializeOwned>(arguments: &Value) -> anyhow::Result<T> {
    serde_json::from_value(arguments.clone()).map_err(|_| anyhow::anyhow!(ARGUMENTS))
}

fn runtime(service: &AgentExecution) -> anyhow::Result<Arc<PluginRuntime>> {
    service.plugins.get().cloned().ok_or_else(|| anyhow::anyhow!(NOT_NOW))
}

/// What one `plugins.*` call answers, or the fixed sentence the model reads when it is refused.
pub(in crate::agent_execution) fn prepare(
    service: &AgentExecution,
    call: &CallContext,
    name: &str,
    arguments: &Value,
) -> Result<Value, &'static str> {
    answer(service, call, name, arguments).map_err(|error| refusal(&error))
}

fn answer(
    service: &AgentExecution,
    call: &CallContext,
    name: &str,
    arguments: &Value,
) -> anyhow::Result<Value> {
    let plugins = runtime(service)?;
    Ok(match name {
        "plugins.validate" => {
            let input: PluginsValidateInput = input(arguments)?;
            let folder = workspace_folder(service, call, &input.path)?;
            let checked =
                aura_plugins::validate_plugin(&folder, &aura_skills::FileHashCache::default());
            let counts = checked.brings;
            let brings = [
                ("skills", counts.skills),
                ("commands", counts.commands),
                ("servers", counts.servers),
                ("hooks", counts.hooks),
                ("panels", counts.panels),
                ("services", counts.services),
                ("channels", counts.channels),
                ("themes", counts.themes),
            ]
            .into_iter()
            .filter(|(_, count)| *count > 0)
            .map(|(kind, count)| (kind.to_owned(), count))
            .collect();
            serde_json::to_value(PluginsValidateResult {
                name: checked.name,
                version: checked.version,
                installable: checked.installable,
                problems: checked.problems,
                warnings: checked.warnings,
                brings,
            })?
        }
        "plugins.dev_install" => {
            let input: PluginsDevInstallInput = input(arguments)?;
            let folder = workspace_folder(service, call, &input.path)?;
            serde_json::to_value(match plugins.install_development(&folder) {
                Ok(view) => {
                    let state = serde_json::to_value(view.state)?.as_str().map(str::to_owned);
                    PluginsDevInstallResult {
                        installed: true,
                        next: next_step(state.as_deref()).into(),
                        name: Some(view.name),
                        folder: view.development,
                        state,
                        problems: Vec::new(),
                    }
                }
                Err(error) => PluginsDevInstallResult {
                    installed: false,
                    name: None,
                    folder: None,
                    state: None,
                    problems: problems(&error),
                    next: "没有装上：按 problems 改好，用 plugins.validate 检查后再装。".into(),
                },
            })?
        }
        "plugins.logs" => {
            let input: PluginsLogsInput = input(arguments)?;
            let limit = input
                .limit
                .map_or(DEFAULT_LOGS, |limit| usize::try_from(limit).unwrap_or(MAX_LOGS))
                .clamp(1, MAX_LOGS);
            let (entries, truncated) =
                plugins.recent_logs(&input.name, limit).map_err(|_| anyhow::anyhow!(NOT_FOUND))?;
            serde_json::to_value(PluginsLogsResult { name: input.name, entries, truncated })?
        }
        _ => anyhow::bail!(ARGUMENTS),
    })
}

/// What the user does next, by the state the plugin is in.
fn next_step(state: Option<&str>) -> &'static str {
    match state {
        Some("ready") => {
            "已装着，用户已信任：Skill、命令、面板和主题改了会自动重新载入；清单、Hook、MCP 服务器和后台服务改了，要用户在 设置 → 插件 里重新信任。"
        }
        Some("content_changed") => {
            "已装着，有改动要用户在 设置 → 插件 里重新信任后才生效；请告诉用户。"
        }
        Some("invalid" | "unavailable") => {
            "已装着，但现在读不出来：用 plugins.validate 检查并改好。"
        }
        _ => {
            "已作为开发中的插件装上，还没有信任：请用户在 设置 → 插件 里查看并信任它；它要的能力和设置由用户在那里决定。"
        }
    }
}

/// The reasons in a refusal, without its code.
fn problems(error: &anyhow::Error) -> Vec<String> {
    let text = error.to_string();
    let reasons = text.split_once(": ").map_or(text.as_str(), |(_, reasons)| reasons);
    reasons.split('；').map(str::to_owned).filter(|reason| !reason.is_empty()).collect()
}

/// `path` as a folder of this workspace, where it really is: relative, inside the root after
/// links are followed, and itself a folder.
fn workspace_folder(
    service: &AgentExecution,
    call: &CallContext,
    path: &str,
) -> anyhow::Result<PathBuf> {
    let relative =
        aura_domain_common::WorkspacePath::parse(path.trim().trim_end_matches(['/', '\\']))
            .map_err(|_| anyhow::anyhow!(OUTSIDE))?;
    let resolver = service.workspace.resolver(call.snapshot.workspace_id)?;
    resolver.verify_root_identity()?;
    let root = resolver.canonical_root().to_path_buf();
    let folder = std::fs::canonicalize(root.join(relative.as_str().replace('/', "\\")))
        .map_err(|_| anyhow::anyhow!(OUTSIDE))?;
    let root = std::fs::canonicalize(&root)?;
    anyhow::ensure!(folder.starts_with(&root) && folder.is_dir(), OUTSIDE);
    Ok(folder)
}
