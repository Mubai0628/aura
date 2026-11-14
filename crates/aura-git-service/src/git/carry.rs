//! The reads behind taking the main checkout's uncommitted changes into a new worktree and
//! writing a worktree's own changes back into the checkout's files: which files differ from a
//! commit, how large they are, and a branch's changes with each side's blob.
use super::*;

/// One file a branch changed: what it was and what it is, each as a mode and a blob.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BranchChange {
    pub path: String,
    pub before: Blob,
    pub after: Blob,
}

/// One side of a changed file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Blob {
    Absent,
    /// A regular file, executable or not.
    File(String),
    /// A link or a submodule: never written by Aura.
    Other,
}

impl GitReader {
    /// The files of this checkout that differ from `commit`: tracked ones edited or deleted,
    /// staged or not, and new ones that are not ignored, by path. Submodules and nested
    /// repositories are left out.
    pub fn changed_from(&self, commit: &str) -> Result<Vec<String>> {
        let commit = self.oid(commit)?;
        let tracked = self.command(&[
            "diff",
            "--name-only",
            "-z",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
            "--ignore-submodules=all",
            &commit,
        ])?;
        let untracked = self.command(&["ls-files", "--others", "--exclude-standard", "-z"])?;
        let mut paths = std::collections::BTreeSet::new();
        for field in tracked.split(|byte| *byte == 0).chain(untracked.split(|byte| *byte == 0)) {
            // A nested repository is listed as its folder.
            if field.is_empty() || field.ends_with(b"/") {
                continue;
            }
            paths.insert(
                String::from_utf8(field.to_vec())
                    .map_err(|_| config::unreadable_output("path_format", "文件列表"))?,
            );
        }
        Ok(paths.into_iter().collect())
    }

    /// The bytes of the regular files among `files` in this checkout's folder.
    pub fn files_size(&self, files: &[String]) -> Result<u64> {
        let mut total = 0_u64;
        for path in files {
            if let Ok(metadata) = std::fs::symlink_metadata(file_at(&self.root, path)?)
                && metadata.is_file()
            {
                total = total.saturating_add(metadata.len());
            }
        }
        Ok(total)
    }

    /// The files `tip` changed since `base`, each side named by its mode and blob.
    pub(super) fn branch_changes(&self, base: &str, tip: &str) -> Result<Vec<BranchChange>> {
        let output = self.command(&[
            "diff",
            "--raw",
            "-z",
            "--no-abbrev",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
            "--ignore-submodules=all",
            base,
            tip,
        ])?;
        let mut changes = Vec::new();
        let mut fields = output.split(|byte| *byte == 0).filter(|field| !field.is_empty());
        let diff_format = || config::unreadable_output("diff_format", "改动列表");
        while let (Some(meta), Some(path)) = (fields.next(), fields.next()) {
            let meta = std::str::from_utf8(meta).map_err(|_| diff_format())?;
            let parts: Vec<&str> = meta.trim_start_matches(':').split(' ').collect();
            let [old_mode, new_mode, old_oid, new_oid, _status] = parts[..] else {
                return Err(diff_format());
            };
            changes.push(BranchChange {
                path: String::from_utf8(path.to_vec()).map_err(|_| diff_format())?,
                before: blob(old_mode, old_oid),
                after: blob(new_mode, new_oid),
            });
        }
        Ok(changes)
    }

    /// A regular file's blob as a checkout of `path` writes it: line endings and the other
    /// filters of this checkout applied.
    pub(super) fn checked_out(&self, path: &str, oid: &str) -> Result<Vec<u8>> {
        self.command(&["cat-file", "--filters", &format!("--path={path}"), oid])
    }
}

fn blob(mode: &str, oid: &str) -> Blob {
    match mode {
        "000000" => Blob::Absent,
        "100644" | "100755" => Blob::File(oid.to_owned()),
        _ => Blob::Other,
    }
}

/// Where repository path `path` is under `root`. Refused for a path that would leave `root`.
pub(super) fn file_at(root: &Path, path: &str) -> Result<PathBuf> {
    let checked = aura_domain_common::WorkspacePath::parse(path)?;
    ensure!(
        !checked.is_root(),
        "git.path_invalid: 路径指向仓库根目录本身，不是仓库里的文件，没有处理它。请给出仓库里的文件路径。"
    );
    let mut full = root.to_owned();
    for part in path.split('/') {
        full.push(part);
    }
    Ok(full)
}
