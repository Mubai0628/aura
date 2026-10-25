//! Every file in a plugin folder. Links and mount points are refused, `.git` is not part of a
//! plugin, and the fingerprint covers every file's path, size and content in path order. The
//! files Aura reads as text are read in the same
//! pass that hashed them, so what is shown is what the fingerprint covers.
use crate::package::PluginPackageError;
use aura_skills::FileHashCache;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

pub const MAX_PLUGIN_FILES: usize = 4000;
pub const MAX_PLUGIN_BYTES: u64 = 256 * 1024 * 1024;
/// Folders that are never part of a plugin: not read, not copied, not fingerprinted.
pub const SKIPPED_FOLDERS: [&str; 1] = [".git"];
const MAX_DEPTH: usize = 16;
pub(crate) const MAX_TEXT_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct PluginFile {
    /// Relative to the plugin folder, with `/` separators.
    pub path: String,
    pub bytes: u64,
}

pub(crate) struct Walked {
    /// Path, size and BLAKE3, in path order.
    pub files: Vec<(String, u64, String)>,
    pub bytes: u64,
    /// The text of each file `wanted` chose, or why it is not text.
    pub texts: BTreeMap<String, Result<String, String>>,
}

impl Walked {
    pub fn text(&self, relative: &str) -> Option<&Result<String, String>> {
        self.texts.get(relative)
    }

    pub fn has(&self, relative: &str) -> bool {
        self.files.iter().any(|(path, _, _)| path == relative)
    }

    /// Whether any file is under the folder `relative`.
    pub fn has_folder(&self, relative: &str) -> bool {
        let prefix = format!("{relative}/");
        self.files.iter().any(|(path, _, _)| path.starts_with(&prefix))
    }

    pub fn fingerprint(&self) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"aura.plugin.package.v1\n");
        for (path, bytes, hash) in &self.files {
            hasher.update(path.as_bytes());
            hasher.update(b"\0");
            hasher.update(bytes.to_string().as_bytes());
            hasher.update(b"\0");
            hasher.update(hash.as_bytes());
            hasher.update(b"\n");
        }
        hasher.finalize().to_hex().to_string()
    }
}

fn reading(error: std::io::Error) -> PluginPackageError {
    PluginPackageError::Io(error.to_string())
}

/// Reads every file under `root`. `wanted` names (by `/`-separated relative path) the files
/// whose text is kept.
pub(crate) fn walk(
    root: &Path,
    cache: &FileHashCache,
    wanted: &dyn Fn(&str) -> bool,
) -> Result<Walked, PluginPackageError> {
    let metadata = std::fs::symlink_metadata(root).map_err(|_| PluginPackageError::NotAFolder)?;
    if reparse_point(&metadata) {
        return Err(PluginPackageError::Linked(root.display().to_string()));
    }
    if !metadata.is_dir() {
        return Err(PluginPackageError::NotAFolder);
    }
    let mut walked = Walked { files: Vec::new(), bytes: 0, texts: BTreeMap::new() };
    directory(root, root, 0, cache, wanted, &mut walked)?;
    walked.files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(walked)
}

fn directory(
    root: &Path,
    folder: &Path,
    depth: usize,
    cache: &FileHashCache,
    wanted: &dyn Fn(&str) -> bool,
    walked: &mut Walked,
) -> Result<(), PluginPackageError> {
    if depth > MAX_DEPTH {
        return Err(PluginPackageError::TooLarge);
    }
    for entry in std::fs::read_dir(folder).map_err(reading)? {
        let path = entry.map_err(reading)?.path();
        let metadata = std::fs::symlink_metadata(&path).map_err(reading)?;
        let relative = path
            .strip_prefix(root)
            .map_err(|_| PluginPackageError::Linked(path.display().to_string()))?
            .to_string_lossy()
            .replace('\\', "/");
        if reparse_point(&metadata) {
            return Err(PluginPackageError::Linked(relative));
        }
        if metadata.is_dir() {
            let name = path.file_name().map(|name| name.to_string_lossy().into_owned());
            if name.is_some_and(|name| SKIPPED_FOLDERS.iter().any(|s| name.eq_ignore_ascii_case(s)))
            {
                continue;
            }
            directory(root, &path, depth + 1, cache, wanted, walked)?;
        } else if metadata.is_file() {
            file(&path, relative, &metadata, cache, wanted, walked)?;
        }
    }
    Ok(())
}

fn file(
    path: &Path,
    relative: String,
    metadata: &std::fs::Metadata,
    cache: &FileHashCache,
    wanted: &dyn Fn(&str) -> bool,
    walked: &mut Walked,
) -> Result<(), PluginPackageError> {
    if walked.files.len() >= MAX_PLUGIN_FILES {
        return Err(PluginPackageError::TooLarge);
    }
    walked.bytes += metadata.len();
    if walked.bytes > MAX_PLUGIN_BYTES {
        return Err(PluginPackageError::TooLarge);
    }
    let hash = if wanted(&relative) {
        let mut content = Vec::new();
        std::fs::File::open(path)
            .map_err(reading)?
            .take(MAX_TEXT_BYTES + 1)
            .read_to_end(&mut content)
            .map_err(reading)?;
        if content.len() as u64 > MAX_TEXT_BYTES {
            // Too large to read as text; the fingerprint still covers the whole file.
            walked.texts.insert(relative.clone(), Err(format!("超过 {MAX_TEXT_BYTES} 字节")));
            cache.hash(path, metadata).map_err(reading)?
        } else {
            let hash = blake3::hash(&content).to_hex().to_string();
            cache.remember((path.to_path_buf(), metadata.len(), metadata.modified().ok()), &hash);
            let text = String::from_utf8(content)
                .map(|text| text.trim_start_matches('\u{feff}').to_owned())
                .map_err(|_| "不是 UTF-8 文本".to_owned());
            walked.texts.insert(relative.clone(), text);
            hash
        }
    } else {
        cache.hash(path, metadata).map_err(reading)?
    };
    walked.files.push((relative, metadata.len(), hash));
    Ok(())
}

#[cfg(windows)]
pub(crate) fn reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
pub(crate) fn reparse_point(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}
