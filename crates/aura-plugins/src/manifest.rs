//! `aura-plugin.toml`, and what both layouts come to: a name that is the plugin's identity, a
//! version, description and author to show, and the limits its processes run under.
use crate::views::PluginLimits;
use serde::Deserialize;
use std::ops::RangeInclusive;

pub const AURA_MANIFEST: &str = "aura-plugin.toml";
pub const CLAUDE_MANIFEST: &str = ".claude-plugin/plugin.json";
pub(crate) const MAX_MANIFEST_BYTES: usize = 64 * 1024;
const MAX_NAME_CHARS: usize = 32;
const MAX_VERSION_CHARS: usize = 32;
const MAX_DESCRIPTION_CHARS: usize = 2000;
const MAX_LINE_CHARS: usize = 200;

pub const DEFAULT_MEMORY_MB: u32 = 2048;
pub const DEFAULT_PROCESSES: u32 = 64;
pub const MEMORY_MB_RANGE: RangeInclusive<u32> = 64..=16_384;
pub const PROCESSES_RANGE: RangeInclusive<u32> = 1..=512;

impl Default for PluginLimits {
    fn default() -> Self {
        Self { memory_mb: DEFAULT_MEMORY_MB, processes: DEFAULT_PROCESSES }
    }
}

/// A plugin's own description, whichever layout it came in.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PluginManifest {
    /// The plugin's identity: one installed plugin per name.
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub limits: PluginLimits,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuraManifestFile {
    schema_version: u32,
    name: String,
    version: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    homepage: Option<String>,
    #[serde(default)]
    limits: Option<LimitsFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LimitsFile {
    #[serde(default)]
    memory_mb: Option<u32>,
    #[serde(default)]
    processes: Option<u32>,
}

/// Lowercase letters, digits and `-`, starting with a letter, at most 32 characters.
#[must_use]
pub fn valid_plugin_name(name: &str) -> bool {
    (1..=MAX_NAME_CHARS).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// `aura-plugin.toml`, or every reason it cannot be used.
pub(crate) fn parse_aura_manifest(text: &str) -> Result<PluginManifest, Vec<String>> {
    if text.len() > MAX_MANIFEST_BYTES {
        return Err(vec![format!("{AURA_MANIFEST} 超过 {MAX_MANIFEST_BYTES} 字节")]);
    }
    let file: AuraManifestFile =
        toml::from_str(text).map_err(|error| vec![format!("{AURA_MANIFEST} 无法解析：{error}")])?;
    let mut problems = Vec::new();
    if file.schema_version != 1 {
        problems.push(format!(
            "schema_version 是 {}；这个版本的 Aura 只认 1，请更新 Aura",
            file.schema_version
        ));
    }
    let mut limits = PluginLimits::default();
    if let Some(declared) = file.limits {
        if let Some(memory) = declared.memory_mb {
            if MEMORY_MB_RANGE.contains(&memory) {
                limits.memory_mb = memory;
            } else {
                problems.push(format!(
                    "limits.memory_mb 必须在 {} 到 {} 之间",
                    MEMORY_MB_RANGE.start(),
                    MEMORY_MB_RANGE.end()
                ));
            }
        }
        if let Some(processes) = declared.processes {
            if PROCESSES_RANGE.contains(&processes) {
                limits.processes = processes;
            } else {
                problems.push(format!(
                    "limits.processes 必须在 {} 到 {} 之间",
                    PROCESSES_RANGE.start(),
                    PROCESSES_RANGE.end()
                ));
            }
        }
    }
    let manifest = tidy(PluginManifest {
        name: file.name.trim().to_owned(),
        version: file.version.trim().to_owned(),
        description: file.description,
        author: file.author,
        homepage: file.homepage,
        limits,
    });
    problems.extend(check(&manifest));
    if problems.is_empty() { Ok(manifest) } else { Err(problems) }
}

/// What a plugin shows about itself, each on one line and bounded. None of it is a reason to refuse
/// a plugin: real Claude Code descriptions run past 500 characters and over several lines.
pub(crate) fn tidy(mut manifest: PluginManifest) -> PluginManifest {
    manifest.description = one_line(&manifest.description, MAX_DESCRIPTION_CHARS);
    manifest.author = blank(manifest.author.map(|text| one_line(&text, MAX_LINE_CHARS)));
    manifest.homepage = blank(manifest.homepage.map(|text| one_line(&text, MAX_LINE_CHARS)));
    manifest
}

/// `text` with every run of spaces, line breaks and other control characters made one space, cut
/// to `max` characters with `…` after.
fn one_line(text: &str, max: usize) -> String {
    let joined = text
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut chars = joined.chars();
    let mut line: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        line.push('…');
    }
    line
}

pub(crate) fn blank(text: Option<String>) -> Option<String> {
    text.map(|text| text.trim().to_owned()).filter(|text| !text.is_empty())
}

/// What every manifest must be, whichever layout it came in.
pub(crate) fn check(manifest: &PluginManifest) -> Vec<String> {
    let mut problems = Vec::new();
    if !valid_plugin_name(&manifest.name) {
        problems.push(format!(
            "name（{}）只能用小写字母、数字和 -，以字母开头，最多 {MAX_NAME_CHARS} 个字符",
            shown(&manifest.name)
        ));
    }
    line("version", &manifest.version, MAX_VERSION_CHARS, true, &mut problems);
    problems
}

fn line(field: &str, text: &str, max: usize, required: bool, problems: &mut Vec<String>) {
    if required && text.is_empty() {
        problems.push(format!("{field} 不能为空"));
    } else if text.chars().any(char::is_control) {
        problems.push(format!("{field} 必须是一行文字"));
    } else if text.chars().count() > max {
        problems.push(format!("{field} 不能超过 {max} 个字符"));
    }
}

/// A name as it may be shown in a message: one line, bounded.
pub(crate) fn shown(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).take(64).collect()
}
