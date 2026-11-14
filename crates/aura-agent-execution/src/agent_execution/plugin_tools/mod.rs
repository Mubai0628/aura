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

// The model reads one of these fixed sentences when a call is refused: the cause, that nothing
// changed, and what to do. The refusal is recorded as the line, a full stop and a sentence of its
// own, so a line ends without one. The answer must be a fixed text, so each tool names its own
// fields.
const ARGUMENTS_VALIDATE: &str = "plugins.arguments_invalid: plugins.validate 只接受 path（要检查的插件文件夹，相对工作区根目录写），什么都没改";
const ARGUMENTS_INSTALL: &str = "plugins.arguments_invalid: plugins.dev_install 只接受 path（插件所在的文件夹，相对工作区根目录写），什么都没装";
const ARGUMENTS_LOGS: &str = "plugins.arguments_invalid: plugins.logs 只接受 name（插件清单里的名字）和可选的 limit（条数，默认 50，最多 200），什么都没读";
const ARGUMENTS: &str = "plugins.arguments_invalid: 没有这个插件工具，什么都没改。可用的是 plugins.validate、plugins.dev_install 和 plugins.logs";
const OUTSIDE: &str = "plugins.path_invalid: path 不是这个工作区里已有的文件夹，什么都没改。path 要相对工作区根目录写，不能是链接";
const NOT_FOUND: &str = "plugins.not_found: 没有这个名字的插件，什么都没读。名字以插件清单里的为准，可以先用 plugins.validate 看清单里的名字";
const WORKSPACE: &str = "plugins.workspace_unavailable: 这个工作区现在读不了（可能被移动、删除或换了磁盘），什么都没改。请用户确认工作区还在后再试";
const NOT_NOW: &str =
    "plugins.unavailable_now: 插件功能现在没有在运行，什么都没改。请稍后再试；反复出现时告诉用户";

pub(in crate::agent_execution) fn is_plugin_tool(action_kind: &str) -> bool {
    action_kind.starts_with("plugins.")
}

fn refusal(error: &anyhow::Error) -> &'static str {
    let text = error.to_string();
    [
        ARGUMENTS_VALIDATE,
        ARGUMENTS_INSTALL,
        ARGUMENTS_LOGS,
        ARGUMENTS,
        OUTSIDE,
        NOT_FOUND,
        WORKSPACE,
    ]
    .into_iter()
    .find(|known| *known == text)
    .unwrap_or(NOT_NOW)
}

fn input<T: serde::de::DeserializeOwned>(
    arguments: &Value,
    refused: &'static str,
) -> anyhow::Result<T> {
    serde_json::from_value(arguments.clone()).map_err(|_| anyhow::anyhow!(refused))
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
            let input: PluginsValidateInput = input(arguments, ARGUMENTS_VALIDATE)?;
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
            let input: PluginsDevInstallInput = input(arguments, ARGUMENTS_INSTALL)?;
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
                    next: failed_next(&error).into(),
                },
            })?
        }
        "plugins.logs" => {
            let input: PluginsLogsInput = input(arguments, ARGUMENTS_LOGS)?;
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

/// What to do after an install that failed, by the refusal's code: a problem in the folder is
/// fixed there; a refusal about Aura's own plugin state is not.
fn failed_next(error: &anyhow::Error) -> &'static str {
    let text = error.to_string();
    match text.split(':').next().unwrap_or_default() {
        "plugins.busy" => "没有装上：插件现在不能改，请用户重启 Aura 后再装。",
        "plugins.registry_unavailable" => {
            "没有装上：插件的设置记录现在读不了，请用户在 设置 → 插件 里查看，好了以后再装。"
        }
        "plugins.name_taken" => {
            "没有装上：按 problems 先处理同名的插件，或在清单里换个名字，再装。"
        }
        _ => "没有装上：按 problems 改好，用 plugins.validate 检查后再装。",
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
    let resolver = service
        .workspace
        .resolver(call.snapshot.workspace_id)
        .map_err(|_| anyhow::anyhow!(WORKSPACE))?;
    resolver.verify_root_identity().map_err(|_| anyhow::anyhow!(WORKSPACE))?;
    let root = resolver.canonical_root().to_path_buf();
    let folder = std::fs::canonicalize(root.join(relative.as_str().replace('/', "\\")))
        .map_err(|_| anyhow::anyhow!(OUTSIDE))?;
    let root = std::fs::canonicalize(&root).map_err(|_| anyhow::anyhow!(WORKSPACE))?;
    anyhow::ensure!(folder.starts_with(&root) && folder.is_dir(), OUTSIDE);
    Ok(folder)
}

#[cfg(test)]
mod tests {
    use super::{
        ARGUMENTS, ARGUMENTS_INSTALL, ARGUMENTS_LOGS, ARGUMENTS_VALIDATE, NOT_FOUND, NOT_NOW,
        OUTSIDE, WORKSPACE, failed_next, problems, refusal,
    };

    #[test]
    fn every_refusal_names_its_cause_and_says_what_changed() {
        let fixed = [
            ARGUMENTS_VALIDATE,
            ARGUMENTS_INSTALL,
            ARGUMENTS_LOGS,
            ARGUMENTS,
            OUTSIDE,
            NOT_FOUND,
            WORKSPACE,
        ];
        for line in fixed {
            assert_eq!(refusal(&anyhow::anyhow!(line)), line, "each cause keeps its own line");
        }
        for line in fixed.into_iter().chain([NOT_NOW]) {
            let (code, sentence) = line.split_once(": ").expect("a code comes first");
            assert!(code.starts_with("plugins.") && !code.contains(' '), "{line}");
            assert!(sentence.contains("什么都没"), "{line}");
            assert!(line.len() <= 512 && !line.contains(['{', '}']), "{line}");
            assert!(!line.ends_with(['。', '.']), "the record adds its own full stop: {line}");
        }
        assert_eq!(refusal(&anyhow::anyhow!("plugins.unexpected")), NOT_NOW);
    }

    #[test]
    fn a_failed_install_gives_its_reasons_and_the_next_step_for_its_cause() {
        let busy = anyhow::anyhow!(
            "plugins.busy: 之前有一次插件改动中途出错，插件现在不能再改，这次什么都没改。请用户重启 Aura 后再试。"
        );
        assert!(failed_next(&busy).contains("重启 Aura"), "{}", failed_next(&busy));
        assert_eq!(problems(&busy).len(), 1, "the reason is a sentence, not the bare code");
        assert!(!problems(&busy)[0].starts_with("plugins."), "{:?}", problems(&busy));
        let registry = anyhow::anyhow!(
            "plugins.registry_unavailable: 插件的设置记录现在读不了（文件损坏），这次什么都没改。"
        );
        assert!(failed_next(&registry).contains("设置 → 插件"), "{}", failed_next(&registry));
        let taken = anyhow::anyhow!("plugins.name_taken: 已装了同名的插件 demo；先在插件页卸载它");
        assert!(failed_next(&taken).contains("同名"), "{}", failed_next(&taken));
        let invalid =
            anyhow::anyhow!("plugins.invalid: 清单缺少 name；Skill skills/broken 读不出来");
        assert!(failed_next(&invalid).contains("plugins.validate"), "{}", failed_next(&invalid));
        assert_eq!(problems(&invalid), ["清单缺少 name", "Skill skills/broken 读不出来"]);
    }
}
