//! A Hook package is a folder the user chose. Its fingerprint covers the manifest and every
//! file in it, so trust given to one content never carries over to another.
use crate::manifest::{HOOK_MANIFEST_FILE, HookDefinition, package_relative, parse_manifest};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MAX_PACKAGE_FILES: usize = 200;
pub const MAX_PACKAGE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_DEPTH: usize = 8;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct HookPackageFile {
    /// Relative to the package root, with `/` separators.
    pub path: String,
    pub bytes: u64,
}

#[derive(Clone, Debug)]
pub struct HookPackage {
    pub root: PathBuf,
    pub definition: HookDefinition,
    pub files: Vec<HookPackageFile>,
    /// BLAKE3 over every file's path and content, in path order.
    pub fingerprint: String,
}

impl HookPackage {
    /// The program file named by the manifest, inside the package.
    pub fn command_path(&self) -> PathBuf {
        self.root.join(self.definition.command.path().replace('/', "\\"))
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HookPackageError {
    #[error("找不到这个文件夹，或它不是普通文件夹")]
    NotAFolder,
    #[error("文件夹或其中的内容是链接或挂载点：{0}")]
    Linked(String),
    #[error("包里超过 {MAX_PACKAGE_FILES} 个文件或 {MAX_PACKAGE_BYTES} 字节，或层级太深")]
    TooLarge,
    #[error("缺少 {HOOK_MANIFEST_FILE}")]
    MissingManifest,
    #[error("{0}")]
    Invalid(String),
    #[error("读取失败：{0}")]
    Io(String),
}

/// Reads and checks the whole package. Nothing outside `root` is followed or read.
pub fn scan_package(root: &Path) -> Result<HookPackage, HookPackageError> {
    let metadata = std::fs::symlink_metadata(root).map_err(|_| HookPackageError::NotAFolder)?;
    if reparse_point(&metadata) {
        return Err(HookPackageError::Linked(root.display().to_string()));
    }
    if !metadata.is_dir() {
        return Err(HookPackageError::NotAFolder);
    }
    let mut files = Vec::new();
    collect(root, root, 0, &mut files)?;
    files.sort_by(|a, b| a.0.cmp(&b.0));
    if files.iter().map(|(_, bytes, _)| *bytes).sum::<u64>() > MAX_PACKAGE_BYTES {
        return Err(HookPackageError::TooLarge);
    }
    let manifest = files
        .iter()
        .find(|(path, _, _)| path == HOOK_MANIFEST_FILE)
        .ok_or(HookPackageError::MissingManifest)?;
    let text = std::str::from_utf8(&manifest.2)
        .map_err(|_| HookPackageError::Invalid(format!("{HOOK_MANIFEST_FILE} 必须是 UTF-8")))?;
    let definition = parse_manifest(text).map_err(|p| HookPackageError::Invalid(p.join("；")))?;
    let command = definition.command.path().replace('\\', "/");
    if !package_relative(&command) || !files.iter().any(|(path, _, _)| *path == command) {
        return Err(HookPackageError::Invalid(format!("包里没有命令文件 {command}")));
    }
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"aura.hook.package.v1\n");
    for (path, bytes, content) in &files {
        hasher.update(path.as_bytes());
        hasher.update(b"\0");
        hasher.update(bytes.to_string().as_bytes());
        hasher.update(b"\0");
        hasher.update(blake3::hash(content).to_hex().as_bytes());
        hasher.update(b"\n");
    }
    Ok(HookPackage {
        root: root.to_path_buf(),
        definition,
        files: files.into_iter().map(|(path, bytes, _)| HookPackageFile { path, bytes }).collect(),
        fingerprint: hasher.finalize().to_hex().to_string(),
    })
}

fn collect(
    root: &Path,
    directory: &Path,
    depth: usize,
    files: &mut Vec<(String, u64, Vec<u8>)>,
) -> Result<(), HookPackageError> {
    if depth > MAX_DEPTH {
        return Err(HookPackageError::TooLarge);
    }
    let entries = std::fs::read_dir(directory).map_err(|e| HookPackageError::Io(e.to_string()))?;
    for entry in entries {
        let entry = entry.map_err(|e| HookPackageError::Io(e.to_string()))?;
        let path = entry.path();
        let metadata =
            std::fs::symlink_metadata(&path).map_err(|e| HookPackageError::Io(e.to_string()))?;
        let relative = path
            .strip_prefix(root)
            .map_err(|_| HookPackageError::Linked(path.display().to_string()))?
            .to_string_lossy()
            .replace('\\', "/");
        if reparse_point(&metadata) {
            return Err(HookPackageError::Linked(relative));
        }
        if metadata.is_dir() {
            collect(root, &path, depth + 1, files)?;
        } else if metadata.is_file() {
            if files.len() >= MAX_PACKAGE_FILES || metadata.len() > MAX_PACKAGE_BYTES {
                return Err(HookPackageError::TooLarge);
            }
            let content = std::fs::read(&path).map_err(|e| HookPackageError::Io(e.to_string()))?;
            files.push((relative, content.len() as u64, content));
        }
    }
    Ok(())
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
