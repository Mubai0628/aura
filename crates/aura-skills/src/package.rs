//! A Skill is a folder with `SKILL.md` and static files. Its fingerprint covers every file's
//! path and content, so trust given to one content never carries over to another. Nothing
//! outside the folder is followed or read, and the text handed out (`SKILL.md`, a resource) is
//! read in the same pass that fingerprinted it.
use crate::frontmatter::{SkillFrontmatter, parse_skill_markdown};
use crate::hash_cache::FileHashCache;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const SKILL_FILE: &str = "SKILL.md";
pub const MAX_SKILL_MD_BYTES: u64 = 256 * 1024;
pub const MAX_SKILL_FILES: usize = 500;
pub const MAX_SKILL_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_RESOURCE_BYTES: u64 = 1024 * 1024;
const MAX_DEPTH: usize = 8;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub struct SkillFile {
    /// Relative to the Skill folder, with `/` separators.
    pub path: String,
    pub bytes: u64,
}

#[derive(Clone, Debug)]
pub struct SkillPackage {
    pub root: PathBuf,
    pub frontmatter: SkillFrontmatter,
    /// `SKILL.md` as written.
    pub skill_md: String,
    /// The instructions after the front matter.
    pub body: String,
    pub files: Vec<SkillFile>,
    pub bytes: u64,
    /// BLAKE3 over every file's path and content, in path order.
    pub fingerprint: String,
    /// Things that do not stop the Skill from being used.
    pub warnings: Vec<String>,
    /// Relative files `SKILL.md` links to that are not in the folder.
    pub missing_references: Vec<String>,
    /// The resource asked for, when one was.
    pub resource: Option<String>,
}

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum SkillPackageError {
    #[error("找不到这个文件夹，或它不是普通文件夹")]
    NotAFolder,
    #[error("文件夹或其中的内容是链接或挂载点：{0}")]
    Linked(String),
    #[error("超过 {MAX_SKILL_FILES} 个文件或 {MAX_SKILL_BYTES} 字节，或层级太深")]
    TooLarge,
    #[error("缺少 {SKILL_FILE}")]
    MissingSkillFile,
    #[error("{}", .0.join("；"))]
    Invalid(Vec<String>),
    #[error("读取失败：{0}")]
    Io(String),
}

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum SkillResourceError {
    #[error("{0}")]
    Package(#[from] SkillPackageError),
    #[error("资源路径必须是 Skill 文件夹内的相对路径，例如 references/guide.md")]
    Path,
    #[error("Skill 里没有 {0}")]
    NotFound(String),
    #[error("{0} 超过 {MAX_RESOURCE_BYTES} 字节，不能整体读取")]
    TooLarge(String),
    #[error("{0} 不是文本文件")]
    NotText(String),
}

/// A resource path as the model or the user wrote it, checked to stay inside the folder.
pub fn resource_path(relative: &str) -> Option<String> {
    let normalized = relative.trim().replace('\\', "/");
    let parts: Vec<&str> =
        normalized.split('/').filter(|part| !part.is_empty() && *part != ".").collect();
    (!parts.is_empty()
        && !normalized.starts_with('/')
        && !normalized.contains(':')
        && parts.iter().all(|part| *part != ".." && !part.chars().any(char::is_control)))
    .then(|| parts.join("/"))
}

struct Walk<'a> {
    root: &'a Path,
    cache: &'a FileHashCache,
    wanted: Option<&'a str>,
    files: Vec<(String, u64, String)>,
    skill_md: Option<String>,
    resource: Option<Result<String, SkillResourceError>>,
    bytes: u64,
}

fn reading(error: std::io::Error) -> SkillPackageError {
    SkillPackageError::Io(error.to_string())
}

impl Walk<'_> {
    fn read_whole(path: &Path, limit: u64) -> Result<Vec<u8>, SkillPackageError> {
        let mut content = Vec::new();
        std::fs::File::open(path)
            .map_err(reading)?
            .take(limit + 1)
            .read_to_end(&mut content)
            .map_err(reading)?;
        Ok(content)
    }

    fn file(
        &mut self,
        path: &Path,
        relative: String,
        metadata: &std::fs::Metadata,
    ) -> Result<(), SkillPackageError> {
        if self.files.len() >= MAX_SKILL_FILES {
            return Err(SkillPackageError::TooLarge);
        }
        self.bytes += metadata.len();
        if self.bytes > MAX_SKILL_BYTES {
            return Err(SkillPackageError::TooLarge);
        }
        let is_skill = relative == SKILL_FILE;
        let wanted = self.wanted.is_some_and(|w| w.eq_ignore_ascii_case(&relative));
        let hash = if is_skill || wanted {
            let limit = if is_skill { MAX_SKILL_MD_BYTES } else { MAX_RESOURCE_BYTES };
            let content = Self::read_whole(path, limit)?;
            if content.len() as u64 > limit {
                // Too large to hand out: the fingerprint still covers the whole file.
                if is_skill {
                    return Err(SkillPackageError::Invalid(vec![format!(
                        "{SKILL_FILE} 超过 {MAX_SKILL_MD_BYTES} 字节"
                    )]));
                }
                self.resource = Some(Err(SkillResourceError::TooLarge(relative.clone())));
                let hash = self.cache.hash(path, metadata).map_err(reading)?;
                self.files.push((relative, metadata.len(), hash));
                return Ok(());
            }
            let hash = blake3::hash(&content).to_hex().to_string();
            self.cache
                .remember((path.to_path_buf(), metadata.len(), metadata.modified().ok()), &hash);
            if is_skill {
                let text = String::from_utf8(content.clone()).map_err(|_| {
                    SkillPackageError::Invalid(vec![format!("{SKILL_FILE} 必须是 UTF-8")])
                })?;
                self.skill_md = Some(text);
            }
            if wanted {
                self.resource = Some(match String::from_utf8(content) {
                    Ok(text) if !text.contains('\0') => Ok(text),
                    _ => Err(SkillResourceError::NotText(relative.clone())),
                });
            }
            hash
        } else {
            self.cache.hash(path, metadata).map_err(reading)?
        };
        self.files.push((relative, metadata.len(), hash));
        Ok(())
    }

    fn directory(&mut self, directory: &Path, depth: usize) -> Result<(), SkillPackageError> {
        if depth > MAX_DEPTH {
            return Err(SkillPackageError::TooLarge);
        }
        for entry in std::fs::read_dir(directory).map_err(reading)? {
            let path = entry.map_err(reading)?.path();
            let metadata = std::fs::symlink_metadata(&path).map_err(reading)?;
            let relative = path
                .strip_prefix(self.root)
                .map_err(|_| SkillPackageError::Linked(path.display().to_string()))?
                .to_string_lossy()
                .replace('\\', "/");
            if reparse_point(&metadata) {
                return Err(SkillPackageError::Linked(relative));
            }
            if metadata.is_dir() {
                self.directory(&path, depth + 1)?;
            } else if metadata.is_file() {
                self.file(&path, relative, &metadata)?;
            }
        }
        Ok(())
    }
}

/// Relative Markdown links in the instructions (`[text](references/a.md)`).
fn links(body: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find("](") {
        rest = &rest[start + 2..];
        let Some(end) = rest.find(')') else { break };
        let target = rest[..end].split(['#', '?', ' ']).next().unwrap_or_default().trim();
        if !target.is_empty()
            && !target.contains(':')
            && !target.starts_with('/')
            && !target.contains('%')
            && let Some(path) = resource_path(target)
        {
            found.push(path);
        }
        rest = &rest[end..];
    }
    found.sort();
    found.dedup();
    found
}

fn scan(
    root: &Path,
    cache: &FileHashCache,
    wanted: Option<&str>,
) -> Result<(SkillPackage, Option<Result<String, SkillResourceError>>), SkillPackageError> {
    let metadata = std::fs::symlink_metadata(root).map_err(|_| SkillPackageError::NotAFolder)?;
    if reparse_point(&metadata) {
        return Err(SkillPackageError::Linked(root.display().to_string()));
    }
    if !metadata.is_dir() {
        return Err(SkillPackageError::NotAFolder);
    }
    let mut walk =
        Walk { root, cache, wanted, files: Vec::new(), skill_md: None, resource: None, bytes: 0 };
    walk.directory(root, 0)?;
    let skill_md = walk.skill_md.take().ok_or(SkillPackageError::MissingSkillFile)?;
    let parsed = parse_skill_markdown(&skill_md).map_err(SkillPackageError::Invalid)?;
    walk.files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"aura.skill.package.v1\n");
    for (path, bytes, hash) in &walk.files {
        hasher.update(path.as_bytes());
        hasher.update(b"\0");
        hasher.update(bytes.to_string().as_bytes());
        hasher.update(b"\0");
        hasher.update(hash.as_bytes());
        hasher.update(b"\n");
    }
    let mut warnings = parsed.warnings;
    let folder =
        root.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    if parsed.frontmatter.name != folder {
        warnings.push(format!("name（{}）与文件夹名（{folder}）不同", parsed.frontmatter.name));
    }
    let missing_references: Vec<String> = links(&parsed.body)
        .into_iter()
        .filter(|link| {
            !walk.files.iter().any(|(path, _, _)| {
                path.eq_ignore_ascii_case(link)
                    || path
                        .to_ascii_lowercase()
                        .starts_with(&format!("{}/", link.to_ascii_lowercase()))
            })
        })
        .collect();
    let resource = walk.resource.take();
    let package = SkillPackage {
        root: root.to_path_buf(),
        frontmatter: parsed.frontmatter,
        skill_md,
        body: parsed.body,
        files: walk
            .files
            .iter()
            .map(|(path, bytes, _)| SkillFile { path: path.clone(), bytes: *bytes })
            .collect(),
        bytes: walk.bytes,
        fingerprint: hasher.finalize().to_hex().to_string(),
        warnings,
        missing_references,
        resource: None,
    };
    Ok((package, resource))
}

/// Reads and checks the whole Skill.
pub fn scan_skill(root: &Path, cache: &FileHashCache) -> Result<SkillPackage, SkillPackageError> {
    scan(root, cache, None).map(|(package, _)| package)
}

/// Reads one resource in the same pass that fingerprints the Skill, so the text returned is
/// exactly the content the fingerprint covers.
pub fn read_resource(
    root: &Path,
    cache: &FileHashCache,
    relative: &str,
) -> Result<SkillPackage, SkillResourceError> {
    let wanted = resource_path(relative).ok_or(SkillResourceError::Path)?;
    let (mut package, resource) = scan(root, cache, Some(&wanted))?;
    match resource {
        None => Err(SkillResourceError::NotFound(wanted)),
        Some(Err(error)) => Err(error),
        Some(Ok(text)) => {
            package.resource = Some(text);
            Ok(package)
        }
    }
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
