use aura_domain_common::WorkspacePath;
use aura_workspace::CatalogEntry;
use aura_workspace_domain::{CatalogEntryKind, TextEncoding};
use globset::{GlobBuilder, GlobMatcher};

use super::policy::MAX_PATH_GLOB_BYTES;
use crate::FsToolError;

#[derive(Clone, Debug)]
pub(crate) struct SearchCandidate {
    pub(crate) ordinal: usize,
    pub(crate) path: WorkspacePath,
    pub(crate) byte_length: Option<u64>,
    pub(crate) ignored: bool,
    pub(crate) generated: bool,
    pub(crate) catalog_encoding: Option<TextEncoding>,
}

pub(crate) fn freeze_candidates(
    entries: impl IntoIterator<Item = CatalogEntry>,
    root: &WorkspacePath,
    path_glob: Option<&str>,
    include_ignored: bool,
    include_generated: bool,
) -> Result<Vec<SearchCandidate>, FsToolError> {
    if let Some(glob) = path_glob
        && glob.len() > MAX_PATH_GLOB_BYTES
    {
        return Err(FsToolError::InvalidRequest("path_glob exceeds 1024 UTF-8 bytes".into()));
    }
    let glob = path_glob.map(compile_glob).transpose()?;
    let mut selected = Vec::new();
    for entry in entries {
        if entry.kind != CatalogEntryKind::File || entry.hidden_from_search {
            continue;
        }
        if !under_root(&entry.path, root) {
            continue;
        }
        if glob.as_ref().is_some_and(|glob| !glob.matches(&entry.path, root)) {
            continue;
        }
        if entry.ignored && !include_ignored {
            continue;
        }
        if entry.generated && !include_generated {
            continue;
        }
        selected.push(SearchCandidate {
            ordinal: 0,
            path: entry.path,
            byte_length: entry.byte_length,
            ignored: entry.ignored,
            generated: entry.generated,
            catalog_encoding: entry.text_encoding,
        });
    }
    selected.sort_by(|left, right| left.path.cmp(&right.path));
    for (ordinal, candidate) in selected.iter_mut().enumerate() {
        candidate.ordinal = ordinal;
    }
    Ok(selected)
}

/// A `path_glob` as ripgrep reads one: without `/` it names files in any folder (`*.rs`,
/// `*.{ts,tsx}`); with `/` it names a path below the search root (`src/**/*.rs`), or below the
/// workspace root when the search root is not where it starts.
enum PathGlob {
    Name(GlobMatcher),
    Path(GlobMatcher),
}

impl PathGlob {
    fn matches(&self, path: &WorkspacePath, root: &WorkspacePath) -> bool {
        let path = path.as_str();
        match self {
            Self::Name(matcher) => matcher.is_match(path.rsplit('/').next().unwrap_or(path)),
            Self::Path(matcher) => {
                matcher.is_match(path)
                    || !root.is_root()
                        && path
                            .strip_prefix(root.as_str())
                            .and_then(|below| below.strip_prefix('/'))
                            .is_some_and(|below| matcher.is_match(below))
            }
        }
    }
}

fn compile_glob(pattern: &str) -> Result<PathGlob, FsToolError> {
    let glob = GlobBuilder::new(pattern.trim_start_matches("./"))
        .literal_separator(true)
        .backslash_escape(false)
        .build()
        .map_err(|error| FsToolError::InvalidRequest(format!("invalid path_glob: {error}")))?;
    let matcher = glob.compile_matcher();
    Ok(if pattern.contains('/') { PathGlob::Path(matcher) } else { PathGlob::Name(matcher) })
}

fn under_root(path: &WorkspacePath, root: &WorkspacePath) -> bool {
    if root.is_root() {
        return true;
    }
    path == root
        || path.as_str().starts_with(root.as_str())
            && path.as_str().as_bytes().get(root.as_str().len()) == Some(&b'/')
}

#[cfg(test)]
#[path = "candidates_tests.rs"]
mod tests;
