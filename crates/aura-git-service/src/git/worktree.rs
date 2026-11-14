//! A repository's worktrees and what one changed since it started: the reads a writing sub-agent's
//! worktree needs. Adding, merging and removing are Git writes (`writes/worktree.rs`).
use super::*;

/// The branch family Aura makes for the worktrees it adds, and the only one it deletes.
pub const AGENT_BRANCH_PREFIX: &str = "aura/agent/";
/// New files counted line by line, at most; the rest are listed without counts.
const COUNTED_NEW_FILES: usize = 500;
/// A new file larger than this is listed without counts.
const COUNTED_BYTES: u64 = 4 * 1024 * 1024;

/// A worktree to add: a new folder outside the repository, on a new branch from `start`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeSpec {
    pub path: PathBuf,
    pub branch: String,
    /// The commit the branch starts from.
    pub start: String,
}

/// One worktree of a repository, as Git lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
}

/// How bringing a branch in ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MergeOutcome {
    Merged {
        head: String,
    },
    /// These files were written into the checkout without a commit.
    Applied {
        files: Vec<String>,
    },
    /// The branch had nothing the current one lacks.
    UpToDate,
    /// Conflicts in these files; the merge was undone and the checkout is as it was.
    Conflicted {
        files: Vec<String>,
    },
    /// The checkout changed these files too, in ways that clash with the branch; nothing was
    /// written.
    Clashed {
        files: Vec<String>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
}

/// One file changed since a worktree started.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub kind: ChangeKind,
    /// Lines added and removed; none for a binary or very large file.
    pub added: Option<u64>,
    pub removed: Option<u64>,
}

impl GitReader {
    pub fn head_commit(&self) -> Result<String> {
        self.oid("HEAD")
    }

    pub fn worktrees(&self) -> Result<Vec<WorktreeEntry>> {
        // Fields end with NUL: a path is then given as it is, never quoted.
        let output =
            String::from_utf8(self.command(&["worktree", "list", "--porcelain", "-z"])?)?;
        let mut entries = Vec::new();
        for line in output.split('\0').filter(|line| !line.is_empty()) {
            if let Some(path) = line.strip_prefix("worktree ") {
                entries.push(WorktreeEntry {
                    path: PathBuf::from(path.replace('/', "\\")),
                    head: None,
                    branch: None,
                });
            } else if let Some(entry) = entries.last_mut() {
                if let Some(head) = line.strip_prefix("HEAD ") {
                    entry.head = Some(head.to_owned());
                } else if let Some(branch) = line.strip_prefix("branch ") {
                    entry.branch =
                        Some(branch.strip_prefix("refs/heads/").unwrap_or(branch).to_owned());
                }
            }
        }
        Ok(entries)
    }

    /// Tracked files with changes not committed, staged or not.
    pub fn uncommitted_paths(&self) -> Result<Vec<String>> {
        let output = self.command(&[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=no",
            "--ignore-submodules=dirty",
        ])?;
        let mut paths = Vec::new();
        let mut fields = output.split(|byte| *byte == 0).filter(|field| !field.is_empty());
        while let Some(field) = fields.next() {
            ensure!(field.len() > 3, "git.status_format: Git 状态格式无法识别");
            let renamed = matches!(field[0], b'R' | b'C') || matches!(field[1], b'R' | b'C');
            paths.push(String::from_utf8(field[3..].to_vec())?);
            if renamed {
                // The name it had: listed with the new one.
                fields.next();
            }
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }

    /// The bytes a checkout of `commit` writes: the sizes of its tracked files.
    pub fn checkout_size(&self, commit: &str) -> Result<u64> {
        let commit = self.oid(commit)?;
        let output = self.command(&["ls-tree", "-r", "-l", "-z", &commit])?;
        let mut total = 0_u64;
        for record in output.split(|byte| *byte == 0).filter(|record| !record.is_empty()) {
            let text = std::str::from_utf8(record)?;
            let (meta, _) = text
                .split_once('\t')
                .ok_or_else(|| anyhow::anyhow!("git.tree_format: Git 文件列表格式无法识别"))?;
            let mut parts = meta.split_whitespace();
            let (_, kind, _, size) = (parts.next(), parts.next(), parts.next(), parts.next());
            if kind == Some("blob") {
                total = total.saturating_add(size.unwrap_or("0").parse().unwrap_or(0));
            }
        }
        Ok(total)
    }

    /// What this checkout changed since `base`: commits, uncommitted edits and new files that are
    /// not ignored, by path.
    pub fn changes_since(&self, base: &str) -> Result<Vec<FileChange>> {
        let base = self.oid(base)?;
        let common = ["--no-renames", "--no-ext-diff", "--no-textconv", "--ignore-submodules=all"];
        let mut kinds = std::collections::BTreeMap::new();
        let mut args = vec!["diff", "--name-status", "-z"];
        args.extend(common);
        args.push(&base);
        let names = self.command(&args)?;
        let mut fields = names.split(|byte| *byte == 0).filter(|field| !field.is_empty());
        while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
            let kind = match status.first() {
                Some(b'A') => ChangeKind::Added,
                Some(b'D') => ChangeKind::Deleted,
                _ => ChangeKind::Modified,
            };
            kinds.insert(String::from_utf8(path.to_vec())?, kind);
        }
        let mut args = vec!["diff", "--numstat", "-z"];
        args.extend(common);
        args.push(&base);
        let counts = self.command(&args)?;
        let mut changes = std::collections::BTreeMap::new();
        for record in counts.split(|byte| *byte == 0).filter(|record| !record.is_empty()) {
            let text = String::from_utf8(record.to_vec())?;
            let mut parts = text.splitn(3, '\t');
            let (Some(added), Some(removed), Some(path)) =
                (parts.next(), parts.next(), parts.next())
            else {
                anyhow::bail!("git.diff_format: Git 改动统计格式无法识别");
            };
            let kind = kinds.get(path).copied().unwrap_or(ChangeKind::Modified);
            changes.insert(
                path.to_owned(),
                FileChange {
                    path: path.to_owned(),
                    kind,
                    added: added.parse().ok(),
                    removed: removed.parse().ok(),
                },
            );
        }
        let untracked = self.command(&["ls-files", "--others", "--exclude-standard", "-z"])?;
        for (index, path) in
            untracked.split(|byte| *byte == 0).filter(|path| !path.is_empty()).enumerate()
        {
            let path = String::from_utf8(path.to_vec())?;
            let added = if index < COUNTED_NEW_FILES { self.new_file_lines(&path) } else { None };
            changes.insert(
                path.clone(),
                FileChange { path, kind: ChangeKind::Added, added, removed: added.map(|_| 0) },
            );
        }
        Ok(changes.into_values().collect())
    }

    /// The lines of a new file, unless it is binary or very large.
    fn new_file_lines(&self, path: &str) -> Option<u64> {
        let full = self.root.join(path.replace('/', "\\"));
        if std::fs::metadata(&full).ok()?.len() > COUNTED_BYTES {
            return None;
        }
        let bytes = std::fs::read(full).ok()?;
        if bytes.iter().take(8000).any(|byte| *byte == 0) {
            return None;
        }
        let breaks = bytes.iter().filter(|byte| **byte == b'\n').count() as u64;
        Some(breaks + u64::from(!bytes.is_empty() && bytes.last() != Some(&b'\n')))
    }
}

/// Whether two paths name the same folder: as written, or after both resolve.
pub(super) fn same_path(left: &Path, right: &Path) -> bool {
    let lexical = |path: &Path| {
        let text = path.to_string_lossy().replace('/', "\\");
        let text = text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned();
        text.trim_end_matches('\\').to_lowercase()
    };
    if lexical(left) == lexical(right) {
        return true;
    }
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => lexical(&left) == lexical(&right),
        _ => false,
    }
}

/// Whether `path` is `folder` or inside it, as written or after the folder resolves.
pub(super) fn inside(path: &Path, folder: &Path) -> bool {
    let lexical = |path: &Path| {
        let text = path.to_string_lossy().replace('/', "\\");
        let text = text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned();
        format!("{}\\", text.trim_end_matches('\\').to_lowercase())
    };
    let target = lexical(path);
    let resolved = std::fs::canonicalize(folder).ok();
    std::iter::once(folder.to_owned())
        .chain(resolved)
        .any(|folder| target.starts_with(&lexical(&folder)))
}

/// A branch Aura made for a worktree: the only family it adds and deletes.
pub(super) fn check_agent_branch(branch: &str) -> Result<()> {
    let name = branch.strip_prefix(AGENT_BRANCH_PREFIX).unwrap_or_default();
    ensure!(
        !name.is_empty()
            && name.len() <= 64
            && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'),
        "git.worktree_branch: 工作副本只用 {AGENT_BRANCH_PREFIX} 下 Aura 建的分支"
    );
    Ok(())
}
