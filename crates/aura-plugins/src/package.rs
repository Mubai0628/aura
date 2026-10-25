//! Reading a whole plugin: its files and fingerprint, its manifest in either layout, and what it
//! brings, each part with its own problems. A part with a problem is shown and not offered; a
//! plugin whose manifest or layout cannot be used is not a plugin Aura can trust.
use crate::claude::{ClaudeServers, parse_plugin_json};
use crate::commands::{PluginCommand, aura_commands, claude_commands};
use crate::files::{MAX_PLUGIN_BYTES, MAX_PLUGIN_FILES, PluginFile, Walked, walk};
use crate::manifest::{AURA_MANIFEST, CLAUDE_MANIFEST, PluginManifest, parse_aura_manifest};
use crate::servers::{
    PluginAuth, PluginServer, PluginTransport, SERVERS_FILE, aura_servers, claude_servers,
};
use crate::views::{
    PluginCommandItem, PluginCounts, PluginHookItem, PluginLayout, PluginServerItem,
    PluginServerKind, PluginSignIn, PluginSkillItem,
};
use aura_hooks::{HookPackage, scan_package};
use aura_skills::{FileHashCache, FoundSkill, scan_root};
use std::path::{Path, PathBuf};

const MAX_PLUGIN_HOOKS: usize = 16;

#[derive(Clone, Debug)]
pub struct PluginHook {
    /// The package folder's name under `hooks`.
    pub folder: String,
    pub package: Result<HookPackage, String>,
}

#[derive(Clone, Debug)]
pub struct PluginPackage {
    pub root: PathBuf,
    pub layout: PluginLayout,
    pub manifest: PluginManifest,
    pub files: Vec<PluginFile>,
    pub bytes: u64,
    /// BLAKE3 over every file's path, size and content, in path order.
    pub fingerprint: String,
    pub skills: Vec<FoundSkill>,
    pub commands: Vec<PluginCommand>,
    pub hooks: Vec<PluginHook>,
    pub servers: Vec<PluginServer>,
    /// Parts Aura does not use, each with the reason.
    pub unsupported: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum PluginPackageError {
    #[error("找不到这个文件夹，或它不是普通文件夹")]
    NotAFolder,
    #[error("文件夹或其中的内容是链接或挂载点：{0}")]
    Linked(String),
    #[error("超过 {MAX_PLUGIN_FILES} 个文件或 {MAX_PLUGIN_BYTES} 字节，或层级太深")]
    TooLarge,
    #[error("不是插件：最外层没有 {AURA_MANIFEST} 或 {CLAUDE_MANIFEST}")]
    NotAPlugin,
    #[error("{}", .0.join("；"))]
    Invalid(Vec<String>),
    #[error("读取失败：{0}")]
    Io(String),
}

/// The files read as text in the same pass that hashes them. Skills and Hook packages are read
/// by their own scanners.
fn wanted(relative: &str) -> bool {
    let lower = relative.to_ascii_lowercase();
    [".md", ".json", ".toml"].iter().any(|ext| lower.ends_with(ext))
        && !lower.starts_with("skills/")
        && !lower.starts_with("hooks/")
        && !lower.split('/').any(|part| part == "node_modules")
}

fn text<'a>(walked: &'a Walked, relative: &str) -> Result<&'a str, String> {
    match walked.text(relative) {
        Some(Ok(text)) => Ok(text),
        Some(Err(reason)) => Err(format!("{relative} 无法读取：{reason}")),
        None => Err(format!("{relative} 无法读取")),
    }
}

/// Reads and checks the plugin in `root`. Nothing is run.
pub fn scan_plugin(
    root: &Path,
    cache: &FileHashCache,
) -> Result<PluginPackage, PluginPackageError> {
    let walked = walk(root, cache, &wanted)?;
    let mut warnings = Vec::new();
    let mut unsupported = Vec::new();
    let (layout, manifest, commands, servers) = if walked.has(AURA_MANIFEST) {
        if walked.has(CLAUDE_MANIFEST) {
            warnings.push(format!("同时有 {CLAUDE_MANIFEST}，按 {AURA_MANIFEST} 读取"));
        }
        let manifest = text(&walked, AURA_MANIFEST)
            .map_err(|problem| vec![problem])
            .and_then(parse_aura_manifest)
            .map_err(PluginPackageError::Invalid)?;
        let servers = if walked.has(SERVERS_FILE) {
            text(&walked, SERVERS_FILE)
                .and_then(|text| aura_servers(text, &manifest.name, root))
                .map_err(|problem| PluginPackageError::Invalid(vec![problem]))?
        } else {
            Vec::new()
        };
        (PluginLayout::Aura, manifest, aura_commands(root), servers)
    } else if walked.has(CLAUDE_MANIFEST) {
        let plugin = text(&walked, CLAUDE_MANIFEST)
            .map_err(|problem| vec![problem])
            .and_then(|text| parse_plugin_json(text, &walked))
            .map_err(PluginPackageError::Invalid)?;
        unsupported.extend(plugin.unsupported);
        let commands = claude_commands(&walked, root, &plugin.command_sources, &mut unsupported);
        let invalid = |problem: String| PluginPackageError::Invalid(vec![problem]);
        let servers = match plugin.servers {
            ClaudeServers::None => Vec::new(),
            ClaudeServers::Inline(value) => {
                claude_servers(&value, &plugin.manifest.name, root, &mut unsupported)
                    .map_err(invalid)?
            }
            ClaudeServers::File(path) => {
                let value: serde_json::Value = text(&walked, &path)
                    .and_then(|text| {
                        serde_json::from_str(text)
                            .map_err(|error| format!("{path} 无法解析：{error}"))
                    })
                    .map_err(invalid)?;
                claude_servers(&value, &plugin.manifest.name, root, &mut unsupported)
                    .map_err(invalid)?
            }
        };
        (PluginLayout::ClaudeCode, plugin.manifest, commands, servers)
    } else {
        return Err(PluginPackageError::NotAPlugin);
    };
    let skills = if root.join("skills").is_dir() {
        scan_root(&root.join("skills"), cache)
    } else {
        Vec::new()
    };
    let hooks = if layout == PluginLayout::Aura { hooks(root) } else { Vec::new() };
    if skills.is_empty() && commands.is_empty() && hooks.is_empty() && servers.is_empty() {
        return Err(PluginPackageError::Invalid(vec![
            "没有 Aura 能用的内容（Skill、命令、MCP 服务器或 Hook）".into(),
        ]));
    }
    Ok(PluginPackage {
        root: root.to_path_buf(),
        layout,
        manifest,
        files: walked
            .files
            .iter()
            .map(|(path, bytes, _)| PluginFile { path: path.clone(), bytes: *bytes })
            .collect(),
        bytes: walked.bytes,
        fingerprint: walked.fingerprint(),
        skills,
        commands,
        hooks,
        servers,
        unsupported,
        warnings,
    })
}

/// Each folder in `hooks` is one Hook package.
fn hooks(root: &Path) -> Vec<PluginHook> {
    let Ok(entries) = std::fs::read_dir(root.join("hooks")) else { return Vec::new() };
    let mut folders: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .map(|entry| (entry.file_name().to_string_lossy().into_owned(), entry.path()))
        .filter(|(name, _)| !name.starts_with('.'))
        .collect();
    folders.sort();
    folders
        .into_iter()
        .enumerate()
        .map(|(index, (folder, path))| PluginHook {
            package: if index < MAX_PLUGIN_HOOKS {
                scan_package(&path).map_err(|error| error.to_string())
            } else {
                Err(format!("超过 {MAX_PLUGIN_HOOKS} 个 Hook，未读取"))
            },
            folder,
        })
        .collect()
}

impl PluginPackage {
    /// The Skills, commands, servers and Hooks it brings, for review.
    #[must_use]
    pub fn skill_items(&self) -> Vec<PluginSkillItem> {
        self.skills
            .iter()
            .map(|found| match &found.scanned {
                Ok(skill) => PluginSkillItem {
                    relative: found.relative.clone(),
                    name: Some(skill.frontmatter.name.clone()),
                    description: Some(skill.frontmatter.description.clone()),
                    problems: Vec::new(),
                },
                Err(error) => PluginSkillItem {
                    relative: found.relative.clone(),
                    name: None,
                    description: None,
                    problems: vec![error.to_string()],
                },
            })
            .collect()
    }

    #[must_use]
    pub fn command_items(&self) -> Vec<PluginCommandItem> {
        self.commands
            .iter()
            .map(|plugin| PluginCommandItem {
                name: plugin.command.name.clone(),
                title: plugin.command.title.clone(),
                description: plugin.command.description.clone(),
                argument_hint: plugin.command.argument_hint.clone(),
                warnings: plugin.warnings.clone(),
                problem: plugin.command.issue.clone(),
            })
            .collect()
    }

    #[must_use]
    pub fn server_items(&self) -> Vec<PluginServerItem> {
        self.servers
            .iter()
            .map(|server| {
                let (kind, target, arguments, sign_in) = match &server.transport {
                    PluginTransport::Stdio { command, arguments, .. } => {
                        (PluginServerKind::Stdio, command.clone(), arguments.clone(), None)
                    }
                    PluginTransport::Http { url, auth, .. } => (
                        PluginServerKind::Http,
                        url.clone(),
                        Vec::new(),
                        auth.as_ref().map(|auth| match auth {
                            PluginAuth::Secret { .. } => PluginSignIn::Secret,
                            PluginAuth::OAuth { .. } => PluginSignIn::Oauth,
                        }),
                    ),
                };
                let secret_names = match &server.transport {
                    PluginTransport::Stdio { secret_environment, .. } => secret_environment.clone(),
                    PluginTransport::Http { .. } => Vec::new(),
                };
                PluginServerItem {
                    name: server.name.clone(),
                    id: server.id.clone(),
                    display_name: server.display_name.clone(),
                    kind,
                    target,
                    arguments,
                    secret_names,
                    sign_in,
                    problems: server.problems.clone(),
                }
            })
            .collect()
    }

    #[must_use]
    pub fn hook_items(&self) -> Vec<PluginHookItem> {
        self.hooks
            .iter()
            .map(|hook| PluginHookItem {
                folder: hook.folder.clone(),
                definition: hook.package.as_ref().ok().map(|package| package.definition.clone()),
                problems: hook.package.as_ref().err().map(|e| vec![e.clone()]).unwrap_or_default(),
            })
            .collect()
    }

    /// How many of each it brings that can be used.
    #[must_use]
    pub fn counts(&self) -> PluginCounts {
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        PluginCounts {
            skills: count(self.skills.iter().filter(|s| s.scanned.is_ok()).count()),
            commands: count(self.commands.iter().filter(|c| c.command.issue.is_none()).count()),
            servers: count(self.servers.iter().filter(|s| s.problems.is_empty()).count()),
            hooks: count(self.hooks.iter().filter(|h| h.package.is_ok()).count()),
        }
    }
}
