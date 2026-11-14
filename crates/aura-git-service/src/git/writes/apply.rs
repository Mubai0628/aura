//! A worktree's own changes brought into the main checkout. With no uncommitted change in the way
//! and the worktree started from the checkout's history, its branch is merged with a commit;
//! otherwise each file it changed is written into the checkout, without a commit, beside the
//! user's own changes. A file both changed in ways that clash stops it before anything is written.
//! Text files are compared and merged by their lines, whatever line endings each checkout gave
//! them, and keep the main checkout's own.
use super::super::carry::{Blob, BranchChange, file_at};
use super::super::worktree::check_agent_branch;
use super::endings;
use super::*;

/// What bringing one changed file in does to the checkout.
enum Step {
    Write(Vec<u8>),
    Delete,
}

/// One side of a file, as a checkout has it.
#[derive(PartialEq, Eq)]
enum Side {
    Absent,
    File(Vec<u8>),
    Other,
}

impl Side {
    fn bytes(&self) -> Option<&[u8]> {
        match self {
            Self::File(bytes) => Some(bytes),
            Self::Absent | Self::Other => None,
        }
    }

    /// The side with each CRLF read as LF.
    fn by_lines(self) -> Self {
        match self {
            Self::File(bytes) => Self::File(endings::to_lf(&bytes)),
            other => other,
        }
    }
}

impl GitWriter {
    /// Brings agent branch `branch`'s changes since `base` into this checkout: merged as
    /// `message` when nothing uncommitted is in the way and `base` is in the checkout's history,
    /// otherwise written into its files without a commit.
    pub fn bring_in(&mut self, branch: &str, base: &str, message: &str) -> Result<MergeOutcome> {
        check_agent_branch(branch)?;
        let base = self.reader.oid(base)?;
        let tip = self.reader.oid(&format!("refs/heads/{branch}"))?;
        let head = self.reader.oid("HEAD")?;
        let changes = self.reader.branch_changes(&base, &tip)?;
        let in_history = self
            .reader
            .raw(&["merge-base".into(), "--is-ancestor".into(), base.clone(), head])?
            .exit_code
            == Some(0);
        if in_history && self.reader.uncommitted_paths()?.is_empty() {
            let untracked =
                self.reader.command(&["ls-files", "--others", "--exclude-standard", "-z"])?;
            let untracked: std::collections::BTreeSet<&[u8]> =
                untracked.split(|byte| *byte == 0).filter(|path| !path.is_empty()).collect();
            if !changes.iter().any(|change| untracked.contains(change.path.as_bytes())) {
                return self.merge(branch, message);
            }
        }
        self.apply(&changes)
    }

    /// Writes each changed file into the checkout: the branch's version where the checkout still
    /// has the one the worktree started from, a three-way merge of the text where both changed.
    fn apply(&mut self, changes: &[BranchChange]) -> Result<MergeOutcome> {
        let git_dir = &self.reader.git_dir;
        ensure!(
            ["MERGE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD", "rebase-merge", "rebase-apply"]
                .iter()
                .all(|name| !git_dir.join(name).exists()),
            "git.operation_in_progress: 合并或其他操作尚未完成；请先在终端处理冲突或完成操作"
        );
        let mut steps = Vec::new();
        let mut clashes = Vec::new();
        for change in changes {
            // Each file has the read time a whole read has.
            self.reader.deadline = std::time::Instant::now() + Duration::from_secs(30);
            let base = self.side(&change.path, &change.before)?;
            let theirs = self.side(&change.path, &change.after)?;
            let ours = on_disk(&file_at(&self.reader.root, &change.path)?)?;
            if [&base, &ours, &theirs].iter().any(|side| **side == Side::Other) {
                clashes.push(change.path.clone());
                continue;
            }
            // A worktree's files may be checked out with other line endings than the checkout's
            // own file has: text is compared and merged by its lines and written with the latter.
            let endings = endings::to_write(ours.bytes(), [theirs.bytes(), base.bytes()]);
            let (base, ours, theirs) = match endings {
                Some(_) => (base.by_lines(), ours.by_lines(), theirs.by_lines()),
                None => (base, ours, theirs),
            };
            let write = |lines: Vec<u8>| match endings {
                Some(endings) => Step::Write(endings.write(lines)),
                None => Step::Write(lines),
            };
            if ours == theirs {
                continue;
            }
            let step = if ours == base {
                match theirs {
                    Side::File(bytes) => Some(write(bytes)),
                    _ => Some(Step::Delete),
                }
            } else if let (Side::File(base), Side::File(ours), Side::File(theirs)) =
                (&base, &ours, &theirs)
            {
                self.merge_text(ours, base, theirs).map(write)
            } else {
                None
            };
            match step {
                Some(step) => steps.push((change.path.clone(), step)),
                None => clashes.push(change.path.clone()),
            }
        }
        if !clashes.is_empty() {
            return Ok(MergeOutcome::Clashed { files: clashes });
        }
        if steps.is_empty() {
            return Ok(MergeOutcome::UpToDate);
        }
        self.begin();
        let mut written: Vec<String> = Vec::new();
        for (path, step) in steps {
            let target = file_at(&self.reader.root, &path)?;
            let done = match step {
                Step::Write(bytes) => write_file(&target, &bytes),
                Step::Delete => std::fs::remove_file(&target),
            };
            if let Err(error) = done {
                let earlier =
                    if written.is_empty() { "无".to_owned() } else { written.join("、") };
                return Err(GitWriteInterrupted(format!(
                    "写进主工作区时 {path} 没写成（{error}）；已写的文件：{earlier}"
                ))
                .into());
            }
            written.push(path);
        }
        Ok(MergeOutcome::Applied { files: written })
    }

    fn side(&self, path: &str, blob: &Blob) -> Result<Side> {
        Ok(match blob {
            Blob::Absent => Side::Absent,
            Blob::File(oid) => Side::File(self.reader.checked_out(path, oid)?),
            Blob::Other => Side::Other,
        })
    }

    /// The text of `ours` and `theirs` merged from `base`; none when they clash, or when a file is
    /// not text Git can merge.
    fn merge_text(&self, ours: &[u8], base: &[u8], theirs: &[u8]) -> Option<Vec<u8>> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let folder = std::env::temp_dir().join(format!(
            "aura-merge-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let merged = (|| -> Result<Option<Vec<u8>>> {
            std::fs::create_dir_all(&folder)?;
            let names = ["ours", "base", "theirs"];
            for (name, bytes) in names.iter().zip([ours, base, theirs]) {
                std::fs::write(folder.join(name), bytes)?;
            }
            let mut args = vec!["merge-file".to_owned(), "-p".into(), "--quiet".into()];
            args.extend(names.iter().map(|name| folder.join(name).display().to_string()));
            let result = self.reader.raw(&args)?;
            Ok((result.exit_code == Some(0)).then_some(result.stdout))
        })();
        let _ = std::fs::remove_dir_all(&folder);
        merged.ok().flatten()
    }
}

fn on_disk(path: &Path) -> Result<Side> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(Side::File(std::fs::read(path)?)),
        Ok(_) => Ok(Side::Other),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Side::Absent),
        Err(error) => Err(error.into()),
    }
}

/// Writes the file whole: a reader sees the old bytes or the new ones, never half.
fn write_file(target: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let name = target.file_name().map(|name| name.to_string_lossy().into_owned());
    let staged = target.with_file_name(format!(
        ".{}.aura-{}.tmp",
        name.unwrap_or_default(),
        std::process::id()
    ));
    std::fs::write(&staged, bytes)?;
    std::fs::rename(&staged, target).inspect_err(|_| {
        let _ = std::fs::remove_file(&staged);
    })
}
