//! Adding a worktree, committing everything in it, merging its branch and removing it.
use super::super::worktree::{check_agent_branch, inside, same_path};
use super::*;

const MESSAGE_MAX_BYTES: usize = 4096;

impl GitWriter {
    /// Adds the worktree, or answers with it when an earlier attempt of the same call added it.
    pub fn add_worktree(&mut self, spec: &WorktreeSpec) -> Result<WorktreeEntry> {
        check_agent_branch(&spec.branch)?;
        ensure!(
            spec.path.is_absolute(),
            "git.worktree_path: 工作副本的位置不是完整路径，没有创建工作副本。请改用完整路径。"
        );
        ensure!(
            !inside(&spec.path, &self.reader.root) && !inside(&spec.path, &self.reader.common_dir),
            "git.worktree_inside: 工作副本不能放在仓库里面，没有创建工作副本。请换一个仓库以外的位置。"
        );
        if let Some(entry) =
            self.reader.worktrees()?.into_iter().find(|entry| same_path(&entry.path, &spec.path))
        {
            ensure!(
                entry.branch.as_deref() == Some(spec.branch.as_str()),
                "git.worktree_taken: 这个位置已经是另一个分支的工作副本，没有创建新的工作副本。请换一个位置。"
            );
            return Ok(entry);
        }
        ensure!(
            std::fs::read_dir(&spec.path).map_or(true, |mut entries| entries.next().is_none()),
            "git.worktree_exists: 这个位置已经有内容不空的文件夹，没有创建工作副本。请换一个位置。"
        );
        let start = self.reader.oid(&spec.start)?;
        let reference = format!("refs/heads/{}", spec.branch);
        let existing = self
            .reader
            .raw(&["show-ref".into(), "--verify".into(), "--quiet".into(), reference])?
            .exit_code
            == Some(0);
        if let Some(parent) = spec.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                anyhow::anyhow!(
                    "git.worktree_folder: 没能建立工作副本所在的文件夹，没有创建工作副本。系统返回：{}",
                    config::shown(&error.to_string())
                )
            })?;
        }
        let path = spec.path.to_string_lossy().into_owned();
        let mut command = vec!["worktree".to_owned(), "add".into()];
        if existing {
            // An earlier attempt made the branch and stopped before its folder.
            command.extend([path, spec.branch.clone()]);
        } else {
            command.extend(["-b".into(), spec.branch.clone(), path, start]);
        }
        self.native(&command)?;
        self.refresh()?;
        let observed = self.reader.worktrees().and_then(|entries| {
            entries
                .into_iter()
                .find(|entry| same_path(&entry.path, &spec.path))
                .ok_or_else(|| anyhow::anyhow!("git.readback_mismatch: Git 列出的工作副本里没有它"))
        });
        observed.map_err(|error| {
            GitWriteInterrupted(format!(
                "git worktree add 已成功退出，工作副本很可能已经建好，但 Aura 没能读回确认（{}）",
                brief(&error)
            ))
            .into()
        })
    }

    /// Commits every change of this checkout that is not ignored; none when there is nothing.
    pub fn commit_all(&mut self, message: &str) -> Result<Option<String>> {
        ensure!(
            !message.trim().is_empty()
                && message.len() <= MESSAGE_MAX_BYTES
                && !message.contains('\0'),
            "git.invalid_message: 提交说明不能为空，最多 4096 字节，且不能含 NUL 字符；这次没有提交。"
        );
        let pending = self.reader.command(&[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=dirty",
        ])?;
        if pending.is_empty() {
            return Ok(None);
        }
        self.native(&["add".into(), "--all".into()])?;
        self.native(&["commit".into(), "-m".into(), message.to_owned()])?;
        self.refresh()?;
        let head = self.reader.oid("HEAD").map_err(|error| {
            GitWriteInterrupted(format!(
                "git commit 已成功退出，提交很可能已经生成，但 Aura 没能读回确认（{}）；不要直接再提交一次，以免重复",
                brief(&error)
            ))
        })?;
        Ok(Some(head))
    }

    /// Merges an agent branch into the current branch. Conflicts are undone and named; uncommitted
    /// changes of this checkout stop it before anything starts.
    pub fn merge(&mut self, branch: &str, message: &str) -> Result<MergeOutcome> {
        check_agent_branch(branch)?;
        ensure!(
            !message.trim().is_empty()
                && message.len() <= MESSAGE_MAX_BYTES
                && !message.contains('\0'),
            "git.invalid_message: 合并说明不能为空，最多 4096 字节，且不能含 NUL 字符；这次没有合并。"
        );
        let pending = self.reader.uncommitted_paths()?;
        if !pending.is_empty() {
            let shown: Vec<&str> = pending.iter().take(20).map(String::as_str).collect();
            anyhow::bail!(
                "git.main_uncommitted: 主工作区有没提交的改动（{}{}），请用户先提交或收起，再合回",
                shown.join("、"),
                if pending.len() > shown.len() { " 等" } else { "" }
            );
        }
        ensure!(
            !self.reader.git_dir.join("MERGE_HEAD").exists(),
            "git.operation_in_progress: 合并或其他操作尚未完成；请先在终端处理冲突或完成操作"
        );
        let before = self.reader.oid("HEAD")?;
        let tip = self.reader.oid(&format!("refs/heads/{branch}"))?;
        let contained = self.reader.raw(&[
            "merge-base".into(),
            "--is-ancestor".into(),
            tip.clone(),
            before.clone(),
        ])?;
        if contained.exit_code == Some(0) {
            return Ok(MergeOutcome::UpToDate);
        }
        let result = self.native_result(&[
            "merge".into(),
            "--no-ff".into(),
            "--no-edit".into(),
            "-m".into(),
            message.to_owned(),
            tip,
        ])?;
        if result.exit_code != Some(0) {
            let conflicted = self
                .reader
                .command(&["diff", "--name-only", "--diff-filter=U", "-z"])
                .unwrap_or_default();
            let files: Vec<String> = conflicted
                .split(|byte| *byte == 0)
                .filter(|path| !path.is_empty())
                .map(|path| String::from_utf8_lossy(path).into_owned())
                .collect();
            if self.reader.git_dir.join("MERGE_HEAD").exists() {
                self.native(&["merge".into(), "--abort".into()])?;
            }
            self.refresh()?;
            let restored = self.reader.oid("HEAD")? == before
                && self.reader.uncommitted_paths()?.is_empty()
                && !self.reader.git_dir.join("MERGE_HEAD").exists();
            if !restored {
                return Err(GitWriteInterrupted(
                    "git merge 没有成功，Aura 撤销合并后，主工作区没能恢复到合并前的样子"
                        .to_owned(),
                )
                .into());
            }
            if !files.is_empty() {
                return Ok(MergeOutcome::Conflicted { files });
            }
            anyhow::bail!("{}", write_failure("merge", result, false));
        }
        self.refresh()?;
        let head = self.reader.oid("HEAD").map_err(|error| {
            GitWriteInterrupted(format!(
                "git merge 已成功退出，合并很可能已经完成，但 Aura 没能读回确认（{}）",
                brief(&error)
            ))
        })?;
        if head == before {
            return Err(GitWriteInterrupted(
                "git merge 已成功退出，但当前分支指向的提交没有变化，无法确认合并结果".to_owned(),
            )
            .into());
        }
        Ok(MergeOutcome::Merged { head })
    }

    /// Removes the worktree's folder with everything in it, and its branch. Gone already is done.
    pub fn remove_worktree(&mut self, path: &Path, branch: &str) -> Result<()> {
        check_agent_branch(branch)?;
        let listed = self.reader.worktrees()?;
        if let Some(entry) = listed.iter().find(|entry| same_path(&entry.path, path)) {
            ensure!(
                entry.branch.is_none() || entry.branch.as_deref() == Some(branch),
                "git.worktree_branch: 这个工作副本不在分支 {branch} 上，没有删除它。"
            );
            self.native(&[
                "worktree".into(),
                "remove".into(),
                "--force".into(),
                path.to_string_lossy().into_owned(),
            ])?;
        }
        self.native(&["worktree".into(), "prune".into()])?;
        let reference = format!("refs/heads/{branch}");
        let exists = self
            .reader
            .raw(&["show-ref".into(), "--verify".into(), "--quiet".into(), reference])?
            .exit_code
            == Some(0);
        if exists {
            self.native(&["branch".into(), "-D".into(), branch.to_owned()])?;
        }
        self.refresh()?;
        ensure!(
            !path.exists(),
            "git.worktree_busy: 工作副本里还有程序在用文件，文件夹（{}）没能删完。请关掉使用这些文件的程序后再删除。",
            path.display()
        );
        Ok(())
    }
}
