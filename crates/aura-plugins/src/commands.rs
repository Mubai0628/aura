//! A plugin's prompt commands: Aura's own command folders (`commands/<name>/`), or a Claude Code
//! plugin's `commands/*.md` (front matter and a template, `$ARGUMENTS` where the typed text
//! goes). Either way a command is only text the user chooses to send. Settings in it that would
//! grant tools or pick a model are ignored, and the preview says so.
use crate::files::Walked;
use aura_agent_domain::{CommandSource, valid_command_name};
use aura_commands::{LoadedCommand, check_template};
use std::path::Path;

const MAX_PLUGIN_COMMANDS: usize = 100;
const MAX_DESCRIPTION_CHARS: usize = 200;
const MAX_HINT_CHARS: usize = 80;

#[derive(Clone, Debug, PartialEq)]
pub struct PluginCommand {
    /// Loaded as the plugin's command (`CommandSource::Plugin`).
    pub command: LoadedCommand,
    pub warnings: Vec<String>,
}

/// An Aura plugin's `commands/<name>/` folders, read by the same rules as the user's.
pub(crate) fn aura_commands(root: &Path) -> Vec<PluginCommand> {
    let directory = root.join("commands");
    if !directory.is_dir() {
        return Vec::new();
    }
    aura_commands::scan(&directory, CommandSource::Plugin)
        .into_iter()
        .take(MAX_PLUGIN_COMMANDS)
        .map(|command| PluginCommand { command, warnings: Vec::new() })
        .collect()
}

/// A Claude Code plugin's commands: the `.md` files directly in each folder of `sources` (or a
/// named `.md` file). Commands in deeper folders are named in `unsupported`.
pub(crate) fn claude_commands(
    walked: &Walked,
    root: &Path,
    sources: &[String],
    unsupported: &mut Vec<String>,
) -> Vec<PluginCommand> {
    let mut found: Vec<&str> = Vec::new();
    let mut nested = std::collections::BTreeSet::new();
    for source in sources {
        for (path, _, _) in &walked.files {
            let lower = path.to_ascii_lowercase();
            if !lower.ends_with(".md") {
                continue;
            }
            if path == source {
                found.push(path);
            } else if let Some(rest) = path.strip_prefix(&format!("{source}/")) {
                match rest.rsplit_once('/') {
                    None => found.push(path),
                    Some((folder, _)) => {
                        nested.insert(format!("{source}/{folder}"));
                    }
                }
            }
        }
    }
    for folder in nested {
        unsupported.push(format!("{folder} 里的命令：子文件夹里的命令没有读取"));
    }
    found.sort_unstable();
    found.dedup();
    found
        .into_iter()
        .take(MAX_PLUGIN_COMMANDS)
        .map(|path| claude_command(walked, root, path))
        .collect()
}

fn claude_command(walked: &Walked, root: &Path, relative: &str) -> PluginCommand {
    let stem = relative.rsplit('/').next().unwrap_or(relative).trim_end_matches(".md");
    let normalized = stem.to_ascii_lowercase().replace('_', "-");
    let mut warnings = Vec::new();
    let mut command = LoadedCommand {
        name: normalized.clone(),
        source: CommandSource::Plugin,
        location: root.join(relative.replace('/', "\\")),
        title: None,
        description: None,
        argument_hint: None,
        template: None,
        manifest: None,
        revision: walked
            .files
            .iter()
            .find(|(path, _, _)| path == relative)
            .map(|(_, _, hash)| hash.clone()),
        issue: None,
    };
    if !valid_command_name(&normalized) {
        command.issue =
            Some(format!("文件名 {stem} 不能作为命令名（只能用小写字母、数字和 -，以字母开头）"));
        return PluginCommand { command, warnings };
    }
    if normalized != stem {
        warnings.push(format!("命令名按 Aura 的写法是 /{normalized}"));
    }
    let text = match walked.text(relative) {
        Some(Ok(text)) => text.replace("\r\n", "\n"),
        Some(Err(reason)) => {
            command.issue = Some(format!("{relative} 无法读取：{reason}"));
            return PluginCommand { command, warnings };
        }
        None => {
            command.issue = Some(format!("{relative} 无法读取"));
            return PluginCommand { command, warnings };
        }
    };
    let (front, body) = split_front_matter(&text);
    for (key, value) in front {
        match key.as_str() {
            "description" => command.description = Some(clip(&value, MAX_DESCRIPTION_CHARS)),
            "argument-hint" => command.argument_hint = Some(clip(&value, MAX_HINT_CHARS)),
            "allowed-tools" => {
                warnings.push("allowed-tools 不生效：Aura 的命令不带任何权限".into())
            }
            "model" => warnings.push("model 不生效：用你在输入框选择的模型".into()),
            _ => {}
        }
    }
    let body = body.trim_start_matches('\n').to_owned();
    if command.description.as_deref().is_none_or(str::is_empty) {
        command.description = body
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(|line| clip(line.trim_start_matches('#').trim(), MAX_DESCRIPTION_CHARS));
    }
    if body.contains("!`") {
        warnings.push("命令里的 !`…` 不会执行，原样作为文字".into());
    }
    if (1..=9).any(|n| body.contains(&format!("${n}"))) {
        warnings.push("位置参数 $1…$9 不会替换，只有 $ARGUMENTS 会".into());
    }
    match check_template(&body) {
        Ok(()) => command.template = Some(body),
        Err(issue) => command.issue = Some(issue),
    }
    PluginCommand { command, warnings }
}

/// `---` front matter: `key: value` lines (quotes removed). List items and anything else are
/// skipped; the rest of the text is the body.
fn split_front_matter(text: &str) -> (Vec<(String, String)>, &str) {
    let Some(rest) = text.strip_prefix("---\n") else { return (Vec::new(), text) };
    let Some(end) = rest.find("\n---") else { return (Vec::new(), text) };
    let after = &rest[end + 4..];
    let body = after.split_once('\n').map_or("", |(_, body)| body);
    let pairs = rest[..end]
        .lines()
        .filter(|line| !line.starts_with([' ', '\t', '-', '#']))
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| {
            let value = value.trim();
            let unquoted = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                .unwrap_or(value);
            (key.trim().to_ascii_lowercase(), unquoted.to_owned())
        })
        .collect();
    (pairs, body)
}

fn clip(text: &str, max: usize) -> String {
    text.chars().filter(|c| !c.is_control()).take(max).collect::<String>().trim().to_owned()
}
