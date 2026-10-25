//! Reading command directories, and the one removal Aura performs itself: a user command in
//! Aura's configuration directory. Workspace commands are workspace files and change only
//! through the ordinary file path.
use super::definition::LoadedCommand;
use super::manifest::{
    MANIFEST_FILE, MAX_MANIFEST_BYTES, MAX_TEMPLATE_BYTES, TEMPLATE_FILE, check_template,
    parse_manifest,
};
use aura_agent_domain::{CommandSource, valid_command_name};
use std::path::Path;

/// Directories read per root; the rest are listed as not read.
pub const MAX_COMMANDS: usize = 200;

#[derive(Debug, thiserror::Error)]
pub enum CommandStoreError {
    #[error("commands.not_found: /{0} 不存在")]
    NotFound(String),
    #[error("commands.revision_changed: /{0} 已被修改，请重新读取后再操作")]
    RevisionChanged(String),
    #[error("commands.not_removable: /{0} 是链接或挂载点，请在资源管理器中处理")]
    NotRemovable(String),
    #[error("commands.io: {0}")]
    Io(#[from] std::io::Error),
}

/// Every command directory under `root`, sorted by name. A missing root has none; names
/// starting with `.` are Aura's own staging and are not commands.
pub fn scan(root: &Path, source: CommandSource) -> Vec<LoadedCommand> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let mut directories = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let metadata = std::fs::symlink_metadata(entry.path()).ok()?;
            (!name.starts_with('.') && (metadata.is_dir() || reparse_point(&metadata)))
                .then(|| (name, entry.path(), reparse_point(&metadata)))
        })
        .collect::<Vec<_>>();
    directories.sort_by(|a, b| a.0.cmp(&b.0));
    directories
        .into_iter()
        .enumerate()
        .map(|(index, (name, path, linked))| {
            if linked {
                LoadedCommand::unusable(name, source, path, "命令目录是链接或挂载点，未读取".into())
            } else if index >= MAX_COMMANDS {
                LoadedCommand::unusable(
                    name,
                    source,
                    path,
                    format!("超过 {MAX_COMMANDS} 个命令，未读取"),
                )
            } else {
                load(&path, source)
            }
        })
        .collect()
}

/// The commands under `base/relative` when every directory on the way is a real directory
/// inside `base`; a linked `.aura` or `.aura/commands` yields none, so nothing outside the
/// workspace is ever read as one of its commands.
pub fn scan_within(base: &Path, relative: &str, source: CommandSource) -> Vec<LoadedCommand> {
    let mut directory = base.to_path_buf();
    for part in relative.split('/') {
        directory.push(part);
        match std::fs::symlink_metadata(&directory) {
            Ok(metadata) if metadata.is_dir() && !reparse_point(&metadata) => {}
            _ => return Vec::new(),
        }
    }
    scan(&directory, source)
}

/// One command directory. Its revision covers whatever files it has, valid or not, so even
/// a broken command can be removed against the version the user saw.
pub fn load(directory: &Path, source: CommandSource) -> LoadedCommand {
    let name = directory.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let manifest = read(&directory.join(MANIFEST_FILE), MAX_MANIFEST_BYTES);
    let template = read(&directory.join(TEMPLATE_FILE), MAX_TEMPLATE_BYTES);
    let revision = revision(manifest.as_ref().ok(), template.as_ref().ok());
    let mut loaded =
        LoadedCommand::unusable(name.clone(), source, directory.to_path_buf(), String::new());
    loaded.revision = Some(revision);
    loaded.manifest = manifest.as_ref().ok().cloned();
    let checked = (|| {
        if !valid_command_name(&name) {
            return Err("目录名只能用小写字母、数字和 -，并以字母开头".to_owned());
        }
        let manifest = parse_manifest(manifest.as_deref()?, &name)?;
        let template = template.as_deref()?;
        check_template(template)?;
        Ok((manifest, template.replace("\r\n", "\n")))
    })();
    match checked {
        Ok((manifest, template)) => {
            loaded.title = Some(manifest.title().to_owned());
            loaded.description = Some(manifest.description.trim().to_owned());
            loaded.argument_hint = manifest
                .argument_hint
                .map(|hint| hint.trim().to_owned())
                .filter(|hint| !hint.is_empty());
            loaded.template = Some(template);
            loaded.issue = None;
        }
        Err(issue) => loaded.issue = Some(issue),
    }
    loaded
}

/// Removes `/name` from the user's commands if it is still the revision the user saw. The
/// directory is first moved aside under a `.` name, so a half-removed directory is never
/// read as a command.
pub fn delete_user_command(
    root: &Path,
    name: &str,
    expected_revision: &str,
) -> Result<(), CommandStoreError> {
    let directory = root.join(name);
    let metadata = std::fs::symlink_metadata(&directory)
        .ok()
        .filter(|_| valid_command_name(name))
        .ok_or_else(|| CommandStoreError::NotFound(name.into()))?;
    if reparse_point(&metadata) || !metadata.is_dir() {
        return Err(CommandStoreError::NotRemovable(name.into()));
    }
    if load(&directory, CommandSource::User).revision.as_deref() != Some(expected_revision) {
        return Err(CommandStoreError::RevisionChanged(name.into()));
    }
    let aside = root.join(format!(".removing-{name}-{}", std::process::id()));
    std::fs::rename(&directory, &aside)?;
    std::fs::remove_dir_all(&aside)?;
    Ok(())
}

fn read(path: &Path, max: usize) -> Result<String, String> {
    let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let metadata = std::fs::symlink_metadata(path).map_err(|_| format!("缺少 {file}"))?;
    if reparse_point(&metadata) || !metadata.is_file() {
        return Err(format!("{file} 必须是普通文件"));
    }
    if metadata.len() > max as u64 {
        return Err(format!("{file} 超过 {max} 字节"));
    }
    let bytes = std::fs::read(path).map_err(|_| format!("{file} 无法读取"))?;
    if bytes.len() > max {
        return Err(format!("{file} 超过 {max} 字节"));
    }
    String::from_utf8(bytes).map_err(|_| format!("{file} 必须是 UTF-8 文本"))
}

fn revision(manifest: Option<&String>, template: Option<&String>) -> String {
    let mut hasher = blake3::Hasher::new();
    for part in [manifest, template] {
        match part {
            Some(text) => {
                hasher.update(&[1]);
                hasher.update(&(text.len() as u64).to_le_bytes());
                hasher.update(text.as_bytes());
            }
            None => {
                hasher.update(&[0]);
            }
        }
    }
    hasher.finalize().to_hex().to_string()
}

#[cfg(windows)]
fn reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn reparse_point(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}
