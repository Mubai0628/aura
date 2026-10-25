//! `command.toml` and `template.md`. Unknown keys and any runtime declaration are refused,
//! so a newer or executable command never runs as a plain prompt.
use aura_agent_domain::valid_command_name;
use serde::{Deserialize, Serialize};

pub const MANIFEST_FILE: &str = "command.toml";
pub const TEMPLATE_FILE: &str = "template.md";
pub const MAX_MANIFEST_BYTES: usize = 4 * 1024;
pub const MAX_TEMPLATE_BYTES: usize = 32 * 1024;
const MAX_TITLE_CHARS: usize = 40;
const MAX_DESCRIPTION_CHARS: usize = 200;
const MAX_HINT_CHARS: usize = 80;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CommandManifest {
    /// The same as the directory name.
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    pub description: String,
    #[serde(default)]
    pub argument_hint: Option<String>,
    pub version: u32,
    /// Reserved for an executable command runtime, which this version does not have.
    #[serde(default)]
    pub runtime: Option<toml::Value>,
}

impl CommandManifest {
    pub fn title(&self) -> &str {
        self.title.as_deref().map_or(self.name.as_str(), str::trim)
    }
}

/// The manifest of the command in directory `directory_name`, or the reason it is unusable.
pub fn parse_manifest(text: &str, directory_name: &str) -> Result<CommandManifest, String> {
    if text.len() > MAX_MANIFEST_BYTES {
        return Err(format!("command.toml 超过 {MAX_MANIFEST_BYTES} 字节"));
    }
    let manifest: CommandManifest =
        toml::from_str(text).map_err(|error| format!("command.toml 无法解析：{error}"))?;
    if manifest.runtime.is_some() {
        return Err("声明了运行时（runtime）；首版只支持提示命令，不执行任何程序".into());
    }
    if manifest.name != directory_name {
        return Err(format!("name 必须与目录名 {directory_name} 相同"));
    }
    if !valid_command_name(&manifest.name) {
        return Err("name 只能用小写字母、数字和 -，并以字母开头".into());
    }
    if manifest.version == 0 {
        return Err("version 必须是正整数".into());
    }
    line("title", manifest.title.as_deref().unwrap_or(&manifest.name), MAX_TITLE_CHARS, true)?;
    line("description", &manifest.description, MAX_DESCRIPTION_CHARS, true)?;
    line("argument_hint", manifest.argument_hint.as_deref().unwrap_or(""), MAX_HINT_CHARS, false)?;
    Ok(manifest)
}

fn line(field: &str, text: &str, max: usize, required: bool) -> Result<(), String> {
    let text = text.trim();
    if required && text.is_empty() {
        return Err(format!("{field} 不能为空"));
    }
    if text.chars().any(char::is_control) {
        return Err(format!("{field} 必须是一行文字"));
    }
    if text.chars().count() > max {
        return Err(format!("{field} 不能超过 {max} 个字符"));
    }
    Ok(())
}

/// A template is ordinary message text: bounded, without control characters or anything
/// that looks like a credential, and linking only to files inside its own directory.
pub fn check_template(text: &str) -> Result<(), String> {
    if text.len() > MAX_TEMPLATE_BYTES {
        return Err(format!("template.md 超过 {MAX_TEMPLATE_BYTES} 字节"));
    }
    if text.trim().is_empty() {
        return Err("template.md 是空的".into());
    }
    if text.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')) {
        return Err("template.md 含有控制字符".into());
    }
    if let Some(kind) = aura_secret_scanner::detect_text(text) {
        return Err(format!("template.md 里像是包含凭据（{kind}），不会提供"));
    }
    for target in link_targets(text) {
        if !inside_directory(target) {
            return Err(format!(
                "template.md 的链接 {target} 不在命令目录内；只能链接本目录里的文件"
            ));
        }
    }
    Ok(())
}

/// Markdown link and image destinations: `](target)`.
fn link_targets(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("](") {
        let after = &rest[start + 2..];
        let target = after.split([')', ' ', '\t', '\n']).next().unwrap_or_default();
        found.push(target.trim_matches(['<', '>']));
        rest = after;
    }
    found
}

/// A relative path that stays inside the command's directory; fragments are fine.
fn inside_directory(target: &str) -> bool {
    if target.is_empty() || target.starts_with('#') {
        return true;
    }
    if target.contains(':') || target.starts_with(['/', '\\']) {
        return false;
    }
    let mut depth = 0usize;
    for part in target.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                if depth == 0 {
                    return false;
                }
                depth -= 1;
            }
            _ => depth += 1,
        }
    }
    true
}

#[derive(Serialize)]
struct ManifestFields<'a> {
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<&'a str>,
    description: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    argument_hint: Option<&'a str>,
    version: u32,
}

/// The `command.toml` Aura writes for a command. It is checked by parsing it back, so a
/// written manifest is always one Aura itself accepts.
pub fn render_manifest(
    name: &str,
    title: Option<&str>,
    description: &str,
    argument_hint: Option<&str>,
    version: u32,
) -> Result<String, String> {
    let text = toml::to_string(&ManifestFields {
        name,
        title: blank(title),
        description: description.trim(),
        argument_hint: blank(argument_hint),
        version,
    })
    .map_err(|error| format!("command.toml 无法生成：{error}"))?;
    parse_manifest(&text, name)?;
    Ok(text)
}

fn blank(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|text| !text.is_empty())
}
