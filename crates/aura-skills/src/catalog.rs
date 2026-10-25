//! Finding Skill folders under one root: `<root>/<skill>/SKILL.md`, or one level deeper in a
//! category folder (`<root>/<category>/<skill>/SKILL.md`). A linked folder is reported, never
//! followed; names starting with `.` are Aura's own staging and are skipped.
use crate::hash_cache::FileHashCache;
use crate::package::{SKILL_FILE, SkillPackage, SkillPackageError, reparse_point, scan_skill};
use std::path::{Path, PathBuf};

/// Skill folders read per root; the rest are listed as not read.
pub const MAX_SKILLS_PER_ROOT: usize = 200;

#[derive(Clone, Debug)]
pub struct FoundSkill {
    /// Relative to the root, `/`-separated.
    pub relative: String,
    pub directory: PathBuf,
    pub scanned: Result<SkillPackage, SkillPackageError>,
}

fn children(directory: &Path) -> Vec<(String, PathBuf, bool)> {
    let Ok(entries) = std::fs::read_dir(directory) else { return Vec::new() };
    let mut found: Vec<_> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let metadata = std::fs::symlink_metadata(entry.path()).ok()?;
            let linked = reparse_point(&metadata);
            (!name.starts_with('.') && (metadata.is_dir() || linked))
                .then(|| (name, entry.path(), linked))
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

fn has_skill_file(directory: &Path) -> bool {
    std::fs::symlink_metadata(directory.join(SKILL_FILE))
        .is_ok_and(|m| m.is_file() || reparse_point(&m))
}

/// Whether `base/relative` can be read as a Skill root.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RootStatus {
    Missing,
    /// A folder on the way is a link or mount point; nothing under it is read.
    Linked,
    Ready,
}

/// Checks every folder from `base` down to `base/relative` (`/`-separated) is a real folder,
/// so a linked `.agents` never turns files elsewhere into Skills.
pub fn root_status(base: &Path, relative: &str) -> RootStatus {
    let mut directory = base.to_path_buf();
    for part in relative.split('/') {
        directory.push(part);
        match std::fs::symlink_metadata(&directory) {
            Ok(metadata) if reparse_point(&metadata) => return RootStatus::Linked,
            Ok(metadata) if metadata.is_dir() => {}
            _ => return RootStatus::Missing,
        }
    }
    RootStatus::Ready
}

/// Every Skill folder under `root`, in name order. A missing root has none.
pub fn scan_root(root: &Path, cache: &FileHashCache) -> Vec<FoundSkill> {
    let mut candidates: Vec<(String, PathBuf, bool)> = Vec::new();
    for (name, path, linked) in children(root) {
        if linked || has_skill_file(&path) {
            candidates.push((name, path, linked));
            continue;
        }
        for (child, nested, linked) in children(&path) {
            if linked || has_skill_file(&nested) {
                candidates.push((format!("{name}/{child}"), nested, linked));
            }
        }
    }
    candidates
        .into_iter()
        .enumerate()
        .map(|(index, (relative, directory, linked))| {
            let scanned = if linked {
                Err(SkillPackageError::Linked(relative.clone()))
            } else if index >= MAX_SKILLS_PER_ROOT {
                Err(SkillPackageError::Io(format!("超过 {MAX_SKILLS_PER_ROOT} 个 Skill，未读取")))
            } else {
                scan_skill(&directory, cache)
            };
            FoundSkill { relative, directory, scanned }
        })
        .collect()
}
