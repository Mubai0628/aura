//! A plugin packed as a ZIP file, read by the same checks as a Skill's (every entry checked
//! before anything is written), with a plugin's limits and root files. A GitHub download, whose
//! files sit in one top-level folder, works as is; a `.git` folder is left out.
use crate::files::{MAX_PLUGIN_BYTES, MAX_PLUGIN_FILES, SKIPPED_FOLDERS};
use crate::manifest::{AURA_MANIFEST, CLAUDE_MANIFEST};
use aura_skills::{ArchiveRules, SkillArchiveError, unpack_archive};
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

/// The largest plugin ZIP file that is read at all.
pub const MAX_PLUGIN_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;

pub const PLUGIN_ARCHIVE: ArchiveRules<'static> = ArchiveRules {
    max_archive_bytes: MAX_PLUGIN_ARCHIVE_BYTES,
    max_files: MAX_PLUGIN_FILES,
    max_bytes: MAX_PLUGIN_BYTES,
    markers: &[AURA_MANIFEST, CLAUDE_MANIFEST],
    skipped_folders: &SKIPPED_FOLDERS,
};

/// Unpacks a plugin archive into `destination`, a new empty folder, and returns the plugin's
/// root. On an error the caller removes `destination`.
pub fn unpack_plugin_archive<R: Read + Seek>(
    source: R,
    destination: &Path,
) -> Result<PathBuf, SkillArchiveError> {
    unpack_archive(source, destination, &PLUGIN_ARCHIVE)
}
