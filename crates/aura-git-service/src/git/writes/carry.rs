//! A new worktree takes the main checkout's uncommitted changes along: they are copied into it
//! and committed on its branch as one commit of their own, so the worktree shows the files the
//! user sees and its own work stays told apart from them.
use super::super::carry::file_at;
use super::super::worktree::check_agent_branch;
use super::*;

/// The most files a new worktree takes along.
pub const CARRY_MAX_FILES: usize = 20_000;
/// The most bytes of files a new worktree takes along.
pub const CARRY_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;

impl GitWriter {
    /// Run in a new worktree still at `start`: copies `files` from the checkout at `from` into
    /// it, deleting the ones gone there, and commits them on its branch as `message`. Returns the
    /// commit its own work starts from: `start` when nothing differed. A worktree no longer at
    /// `start` took them along in an earlier attempt, and that commit is returned.
    ///
    /// The commit is written with Git's plumbing, as Git's own stash writes its commits: no hook
    /// runs and nothing is signed, because the commit only records what the user had not
    /// committed and never enters their branch; the author is Aura.
    pub fn take_along(
        &mut self,
        from: &Path,
        files: &[String],
        start: &str,
        message: &str,
    ) -> Result<String> {
        let start = self.reader.oid(start)?;
        let head = self.reader.oid("HEAD")?;
        if head != start || files.is_empty() {
            return Ok(head);
        }
        let reference = String::from_utf8(self.reader.command(&["symbolic-ref", "-q", "HEAD"])?)?;
        let reference = reference.trim().to_owned();
        check_agent_branch(reference.strip_prefix("refs/heads/").unwrap_or_default())?;
        self.begin();
        for path in files {
            copy_one(&file_at(from, path)?, &file_at(&self.reader.root, path)?).map_err(
                |error| anyhow::anyhow!("git.carry_failed: 没能把 {path} 带进工作副本：{error}"),
            )?;
        }
        self.native(&["add".into(), "--all".into()])?;
        let tree = self.output(&["write-tree".into()])?;
        let before = self.reader.command(&["rev-parse", &format!("{start}^{{tree}}")])?;
        if String::from_utf8(before)?.trim() == tree {
            return Ok(start);
        }
        let commit = self.output(&[
            "-c".into(),
            "user.name=Aura".into(),
            "-c".into(),
            "user.email=aura@localhost".into(),
            "commit-tree".into(),
            tree,
            "-p".into(),
            start.clone(),
            "-m".into(),
            message.to_owned(),
        ])?;
        self.native(&["update-ref".into(), reference, commit.clone(), start])?;
        self.refresh()?;
        let now = self.reader.oid("HEAD").map_err(|error| {
            GitWriteInterrupted(format!(
                "带入的文件已经提交到工作副本的分支，但 Aura 没能读回确认（{}）",
                brief(&error)
            ))
        })?;
        if now != commit {
            return Err(GitWriteInterrupted(
                "带入文件的提交已经生成，但工作副本的分支没有指向它".to_owned(),
            )
            .into());
        }
        Ok(commit)
    }
}

/// Copies one file as the checkout has it, or deletes it where the checkout has none. A folder or
/// a link there is left as the commit has it.
fn copy_one(source: &Path, target: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(source) {
        Ok(metadata) if metadata.is_file() => {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(source, target).map(|_| ())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match std::fs::symlink_metadata(target) {
                Ok(metadata) if metadata.is_file() => std::fs::remove_file(target),
                _ => Ok(()),
            }
        }
        Err(error) => Err(error),
    }
}
