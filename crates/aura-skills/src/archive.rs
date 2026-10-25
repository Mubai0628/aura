//! A Skill packed as a ZIP file (`.zip`, or `.skill` as some tools name it), and under its own
//! rules a plugin. Nothing is written until every entry has been checked: only plain files
//! and folders, paths that stay inside the package, no names Windows would treat as the same,
//! the same size and file limits as a folder, and declared sizes that the bytes really match. A
//! Mac's `__MACOSX` folder and `.DS_Store` files are left out, and so are folders the rules skip
//! (a plugin's `.git`).
use crate::package::{MAX_SKILL_BYTES, MAX_SKILL_FILES, SKILL_FILE};
use std::collections::BTreeSet;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use zip::{CompressionMethod, ZipArchive};

/// The largest Skill ZIP file that is read at all.
pub const MAX_ARCHIVE_BYTES: u64 = 16 * 1024 * 1024;
/// The same compression-ratio limit as a tool package.
const MAX_RATIO: u64 = 100;
const CENTRAL_ENTRY: [u8; 4] = [0x50, 0x4b, 0x01, 0x02];
const CENTRAL_FIXED: usize = 46;
const TYPE_MASK: u32 = 0o170_000;
const REGULAR: u32 = 0o100_000;
const DIRECTORY: u32 = 0o040_000;

/// What one kind of package may hold, and the file that marks its root.
#[derive(Clone, Copy, Debug)]
pub struct ArchiveRules<'a> {
    /// The largest ZIP file that is read at all.
    pub max_archive_bytes: u64,
    pub max_files: usize,
    /// Unpacked bytes of all files together.
    pub max_bytes: u64,
    /// The package's root holds one of these (`/`-separated): at the top of the ZIP, or in its
    /// only top-level folder.
    pub markers: &'a [&'a str],
    /// Folders, by name at any depth, that are not part of the package and are left out.
    pub skipped_folders: &'a [&'a str],
}

/// A Skill: `SKILL.md` at its root, the same limits as a Skill folder.
pub const SKILL_ARCHIVE: ArchiveRules<'static> = ArchiveRules {
    max_archive_bytes: MAX_ARCHIVE_BYTES,
    max_files: MAX_SKILL_FILES,
    max_bytes: MAX_SKILL_BYTES,
    markers: &[SKILL_FILE],
    skipped_folders: &[],
};

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum SkillArchiveError {
    #[error("不是有效的 ZIP 文件")]
    NotZip,
    #[error("ZIP 文件超过 {0} 字节")]
    TooLarge(u64),
    #[error("ZIP 里的条目太多")]
    TooManyEntries,
    #[error("ZIP 里有加密的文件")]
    Encrypted,
    #[error("ZIP 用了不支持的压缩方式（只支持不压缩与 Deflate）")]
    Compression,
    #[error("ZIP 里有链接或特殊文件：{0}")]
    SpecialFile(String),
    #[error("ZIP 里的路径不能用：{0}")]
    Path(String),
    #[error("ZIP 里有在 Windows 上重名的文件或文件夹（不分大小写）：{0}")]
    Duplicate(String),
    #[error("解压后超过 {files} 个文件或 {bytes} 字节")]
    Limits { files: usize, bytes: u64 },
    #[error("ZIP 的压缩比异常")]
    Ratio,
    #[error("{0} 要放在 ZIP 的最外层，或放在唯一的顶层文件夹里")]
    Layout(String),
    #[error("解压失败：{0}")]
    Io(String),
}

fn io(error: impl std::fmt::Display) -> SkillArchiveError {
    SkillArchiveError::Io(error.to_string())
}

struct Entry {
    index: usize,
    name: String,
    path: String,
    size: u64,
    directory: bool,
}

/// Unpacks a Skill archive into `destination`, a new empty folder, and returns the folder that
/// holds its `SKILL.md`: `destination` itself, or its only top-level folder. Nothing outside
/// `destination` is written; on an error the caller removes `destination`.
pub fn unpack_skill_archive<R: Read + Seek>(
    source: R,
    destination: &Path,
) -> Result<PathBuf, SkillArchiveError> {
    unpack_archive(source, destination, &SKILL_ARCHIVE)
}

/// Unpacks a package archive under `rules` into `destination`, a new empty folder, and returns
/// the package's root: `destination` itself, or its only top-level folder.
pub fn unpack_archive<R: Read + Seek>(
    mut source: R,
    destination: &Path,
    rules: &ArchiveRules<'_>,
) -> Result<PathBuf, SkillArchiveError> {
    let length = source.seek(SeekFrom::End(0)).map_err(io)?;
    if length > rules.max_archive_bytes {
        return Err(SkillArchiveError::TooLarge(rules.max_archive_bytes));
    }
    source.seek(SeekFrom::Start(0)).map_err(io)?;
    let archive = ZipArchive::new(source).map_err(|_| SkillArchiveError::NotZip)?;
    let start = archive.central_directory_start();
    let unique = archive.len();
    let mut source = archive.into_inner();
    // The library keeps one entry per name; a raw count that differs means a repeated name.
    if count_central_entries(&mut source, start, max_entries(rules))? != unique {
        return Err(SkillArchiveError::Duplicate("同名条目".into()));
    }
    let mut archive = ZipArchive::new(source).map_err(|_| SkillArchiveError::NotZip)?;
    let entries = preflight(&mut archive, rules)?;
    let root = layout(&entries, rules)?;
    let mut written = 0_u64;
    for entry in &entries {
        let target = destination.join(entry.path.replace('/', "\\"));
        if entry.directory {
            std::fs::create_dir_all(&target).map_err(io)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        let mut input = archive.by_index(entry.index).map_err(io)?;
        if input.name() != entry.name || input.size() != entry.size || input.is_dir() {
            return Err(SkillArchiveError::Io("ZIP 条目在读取时变了".into()));
        }
        let mut output =
            std::fs::OpenOptions::new().write(true).create_new(true).open(&target).map_err(io)?;
        let copied =
            std::io::copy(&mut (&mut input).take(entry.size + 1), &mut output).map_err(io)?;
        output.flush().map_err(io)?;
        // The declared size is what was checked; bytes beyond it are a lie, not content.
        if copied != entry.size {
            return Err(SkillArchiveError::Io("ZIP 条目的实际大小与声明不符".into()));
        }
        written += copied;
        if written > rules.max_bytes {
            return Err(limits(rules));
        }
    }
    Ok(match root {
        Some(folder) => destination.join(folder),
        None => destination.to_path_buf(),
    })
}

/// Files and folders together, before anything is decompressed.
const fn max_entries(rules: &ArchiveRules<'_>) -> usize {
    2 * rules.max_files + 64
}

const fn limits(rules: &ArchiveRules<'_>) -> SkillArchiveError {
    SkillArchiveError::Limits { files: rules.max_files, bytes: rules.max_bytes }
}

fn count_central_entries<R: Read + Seek>(
    source: &mut R,
    start: u64,
    max: usize,
) -> Result<usize, SkillArchiveError> {
    source.seek(SeekFrom::Start(start)).map_err(io)?;
    let mut count = 0_usize;
    loop {
        let mut signature = [0_u8; 4];
        if source.read_exact(&mut signature).is_err() || signature != CENTRAL_ENTRY {
            return Ok(count);
        }
        count += 1;
        if count > max {
            return Err(SkillArchiveError::TooManyEntries);
        }
        let mut fixed = [0_u8; CENTRAL_FIXED - 4];
        source.read_exact(&mut fixed).map_err(|_| SkillArchiveError::NotZip)?;
        let variable = i64::from(u16::from_le_bytes([fixed[24], fixed[25]]))
            + i64::from(u16::from_le_bytes([fixed[26], fixed[27]]))
            + i64::from(u16::from_le_bytes([fixed[28], fixed[29]]));
        source.seek(SeekFrom::Current(variable)).map_err(io)?;
    }
}

fn preflight<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    rules: &ArchiveRules<'_>,
) -> Result<Vec<Entry>, SkillArchiveError> {
    let mut entries = Vec::new();
    // Lower-cased keys: Windows ignores case, so names that differ only in case collide.
    let mut files = BTreeSet::new();
    let mut listed_folders = BTreeSet::new();
    let mut folders = BTreeSet::new();
    let (mut total, mut compressed_total, mut file_count) = (0_u64, 0_u64, 0_usize);
    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index).map_err(|_| SkillArchiveError::NotZip)?;
        let name = entry.name().to_owned();
        let shown: String = name.chars().take(120).collect();
        if entry.encrypted() {
            return Err(SkillArchiveError::Encrypted);
        }
        if !matches!(entry.compression(), CompressionMethod::Stored | CompressionMethod::Deflated) {
            return Err(SkillArchiveError::Compression);
        }
        // Some Windows tools end a folder entry with `\`.
        let directory = entry.is_dir() || (name.ends_with('\\') && entry.size() == 0);
        let kind = entry.unix_mode().map(|mode| mode & TYPE_MASK).unwrap_or(0);
        if entry.is_symlink()
            || (directory && entry.size() != 0)
            || (kind != 0 && kind != if directory { DIRECTORY } else { REGULAR })
        {
            return Err(SkillArchiveError::SpecialFile(shown));
        }
        let Some(path) = entry_path(&name, directory)? else { continue };
        if path.split('/').any(|part| {
            rules.skipped_folders.iter().any(|skipped| part.eq_ignore_ascii_case(skipped))
        }) {
            continue;
        }
        let (size, compressed) = (entry.size(), entry.compressed_size());
        if size > 0 && (compressed == 0 || size > compressed.saturating_mul(MAX_RATIO)) {
            return Err(SkillArchiveError::Ratio);
        }
        let key = path.to_lowercase();
        let parents: Vec<String> =
            key.match_indices('/').map(|(at, _)| key[..at].to_owned()).collect();
        if directory {
            // A folder may be listed once and implied by its files any number of times.
            if files.contains(&key) || !listed_folders.insert(key.clone()) {
                return Err(SkillArchiveError::Duplicate(shown));
            }
            folders.insert(key);
        } else if folders.contains(&key) || !files.insert(key) {
            return Err(SkillArchiveError::Duplicate(shown));
        }
        for parent in parents {
            if files.contains(&parent) {
                return Err(SkillArchiveError::Duplicate(shown));
            }
            folders.insert(parent);
        }
        if !directory {
            file_count += 1;
            total = total.saturating_add(size);
            compressed_total = compressed_total.saturating_add(compressed);
        }
        entries.push(Entry { index, name, path, size, directory });
    }
    if file_count > rules.max_files || total > rules.max_bytes {
        return Err(limits(rules));
    }
    if total > 0 && total > compressed_total.saturating_mul(MAX_RATIO) {
        return Err(SkillArchiveError::Ratio);
    }
    // A file seen before a later entry made its name a folder.
    if entries.iter().any(|e| !e.directory && folders.contains(&e.path.to_lowercase())) {
        return Err(SkillArchiveError::Duplicate("同名的文件与文件夹".into()));
    }
    Ok(entries)
}

/// The entry's path inside the Skill with `/` separators, or `None` for what a Mac adds.
fn entry_path(name: &str, directory: bool) -> Result<Option<String>, SkillArchiveError> {
    let bad = || SkillArchiveError::Path(name.chars().take(120).collect());
    let raw = name.replace('\\', "/");
    let trimmed = if directory { raw.trim_end_matches('/') } else { raw.as_str() };
    if trimmed.is_empty() || trimmed.starts_with('/') || trimmed.contains(':') {
        return Err(bad());
    }
    if !directory && raw.ends_with('/') {
        return Err(bad());
    }
    let mut parts = Vec::new();
    for part in trimmed.split('/') {
        if part == "." {
            continue;
        }
        if part.is_empty()
            || part == ".."
            || part.ends_with('.')
            || part.ends_with(' ')
            || part
                .chars()
                .any(|c| c.is_control() || matches!(c, '<' | '>' | '"' | '|' | '?' | '*'))
            || reserved(part)
        {
            return Err(bad());
        }
        parts.push(part);
    }
    if parts.is_empty() {
        return Err(bad());
    }
    if parts[0] == "__MACOSX" || parts.last() == Some(&".DS_Store") {
        return Ok(None);
    }
    Ok(Some(parts.join("/")))
}

/// Names Windows reserves for devices, with or without an extension.
fn reserved(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or(part).to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit())
}

/// Where the root file is: at the top, or in the only top-level folder.
fn layout(
    entries: &[Entry],
    rules: &ArchiveRules<'_>,
) -> Result<Option<String>, SkillArchiveError> {
    let refused = || SkillArchiveError::Layout(rules.markers.join(" 或 "));
    let holds = |prefix: &str| {
        rules.markers.iter().any(|marker| {
            let wanted = format!("{prefix}{marker}");
            entries.iter().any(|e| !e.directory && e.path == wanted)
        })
    };
    if holds("") {
        return Ok(None);
    }
    let tops: BTreeSet<&str> =
        entries.iter().map(|e| e.path.split('/').next().unwrap_or_default()).collect();
    let [top] = tops.into_iter().collect::<Vec<_>>()[..] else {
        return Err(refused());
    };
    if holds(&format!("{top}/")) { Ok(Some(top.to_owned())) } else { Err(refused()) }
}
