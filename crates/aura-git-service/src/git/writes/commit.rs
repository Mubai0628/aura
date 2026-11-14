use super::*;
use crate::git::status::parse_status;
use aura_workspace_domain::{
    GIT_NATIVE_EXECUTION_NOTICE, GitCommitInput, GitCommitResultV1, GitLogInput, GitStatusInput,
};

#[derive(Debug)]
pub(super) struct EmptyCommit;

impl std::fmt::Display for EmptyCommit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            "git.nothing_to_commit: 没有可提交的改动，没有生成新提交。请用 git.status 核对要提交的文件。",
        )
    }
}

impl std::error::Error for EmptyCommit {}

impl GitWriter {
    pub fn commit(&mut self, input: &GitCommitInput) -> Result<GitCommitResultV1> {
        ensure!(
            !input.message.trim().is_empty()
                && input.message.len() <= 65536
                && !input.message.contains('\0'),
            "git.invalid_message: message（提交说明）不能为空，最多 65536 字节，且不能含 NUL 字符；这次没有提交。请改好 message 再试。"
        );
        ensure!(
            input.staged_only == input.paths.is_empty(),
            "git.invalid_selection: paths 和 staged_only 要二选一：给出要提交的文件（paths），或只提交已暂存的改动（staged_only 为 true），不能都给或都不给；这次没有提交。"
        );
        let mut pathspec = Vec::new();
        paths(&mut pathspec, &input.paths)?;
        let before_status = self.reader.status_output(false)?;
        let before = parse_status(&before_status)?.head;
        let mutation = (|| -> Result<()> {
            let mut add_processes = None;
            if !input.staged_only {
                let mut add = vec!["add".into(), "--all".into()];
                add.extend(pathspec.clone());
                let result = self.native_result(&add)?;
                if result.exit_code != Some(0) {
                    anyhow::bail!("{}", write_failure("add", result, false));
                }
                add_processes = result.total_processes;
            }
            let mut command = vec!["commit".into(), "--message".into(), input.message.clone()];
            if !input.staged_only {
                command.push("--only".into());
                command.extend(pathspec);
            }
            let result = self.native_result(&command)?;
            if result.exit_code == Some(0) {
                return Ok(());
            }
            if reports_empty_commit(&result) {
                self.reader.deadline = std::time::Instant::now() + Duration::from_secs(30);
                if self.reader.status_output(false).is_ok_and(|after| after == before_status)
                    && self.no_commit_helpers(
                        result.total_processes,
                        add_processes,
                        input.staged_only,
                    )
                {
                    return Err(EmptyCommit.into());
                }
            }
            anyhow::bail!("{}", write_failure("commit", result, !input.staged_only))
        })();
        if let Err(error) = mutation {
            return match error.downcast::<GitWriteInterrupted>() {
                Ok(mut interrupted) => {
                    interrupted
                        .0
                        .push_str(&format!("；{}", self.interrupted_head(before.as_deref())));
                    Err(interrupted.into())
                }
                Err(error) => Err(error),
            };
        }
        self.refresh()?;
        let observed = (|| -> Result<_> {
            let status = self.reader.status(&GitStatusInput::default())?;
            let head = status
                .head
                .clone()
                .ok_or_else(|| anyhow::anyhow!("git.readback_failed: 提交后读不到 HEAD"))?;
            let log = self.reader.log(&GitLogInput {
                limit: 1,
                start: None,
                paths: vec![],
                commit: Some(head.clone()),
            })?;
            let commit = log
                .commits
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("git.readback_failed: 读不到新提交的记录"))?;
            ensure!(
                Some(&head) != before.as_ref() && commit.parents.first() == before.as_ref(),
                "git.readback_mismatch: HEAD 在提交期间还被其他操作改变了"
            );
            Ok(GitCommitResultV1 {
                commit,
                branch: status.branch.clone(),
                status,
                execution_notice: GIT_NATIVE_EXECUTION_NOTICE.into(),
            })
        })();
        observed.map_err(|error| {
            GitWriteInterrupted(format!(
                "git commit 已成功退出，提交很可能已经生成，但 Aura 没能读回确认（{}）；不要直接再提交一次，以免重复",
                brief(&error)
            ))
            .into()
        })
    }

    /// Compare the completed native Jobs with an invocation of the same image that
    /// cannot read repository configuration or run Git helpers.
    fn no_commit_helpers(
        &self,
        commit_processes: Option<u32>,
        add_processes: Option<u32>,
        staged_only: bool,
    ) -> bool {
        let Some(count) = commit_processes else { return false };
        if count == 0 || (!staged_only && add_processes != Some(count)) {
            return false;
        }
        // The Windows launcher may bring a console host into the Job even for
        // Git --version. Only the Git process and that platform process are expected.
        let Ok(version) = self.reader.runner.run_native(
            &self.reader.root,
            &["--version".into()],
            &self.reader.cancel,
            Duration::from_secs(3),
            4096,
        ) else {
            return false;
        };
        version.stopped
            && version.reason == "exited"
            && version.exit_code == Some(0)
            && version.total_processes == Some(count)
    }

    fn interrupted_head(&mut self, before: Option<&str>) -> String {
        // The cancellation belongs to the mutation. A separate bounded, isolated read
        // may describe HEAD afterwards, but never turns an interrupted attempt into success.
        let cancelled =
            std::mem::replace(&mut self.reader.cancel, Arc::new(AtomicBool::new(false)));
        self.reader.deadline = std::time::Instant::now() + Duration::from_secs(3);
        let observed = self.reader.raw(&[
            "rev-parse".into(),
            "--verify".into(),
            "--quiet".into(),
            "HEAD".into(),
        ]);
        self.reader.cancel = cancelled;
        match observed {
            Ok(out) if matches!(out.exit_code, Some(0 | 1)) => {
                let text = String::from_utf8_lossy(&out.stdout);
                let current = (out.exit_code == Some(0)).then_some(text.trim());
                if current == before {
                    format!(
                        "中断后 HEAD 未移动（{}），没有生成新提交，但选中的文件可能已经暂存；index.lock 若存在将保留，请检查后再操作",
                        current.unwrap_or("尚无提交")
                    )
                } else {
                    format!(
                        "中断后 HEAD 已变化：{} → {}，提交可能已经生成；请核对历史与 index.lock，不能据此自动重试",
                        before.unwrap_or("尚无提交"),
                        current.unwrap_or("尚无提交")
                    )
                }
            }
            _ => "中断后读不到 HEAD，无法确认是否生成了提交；请手动核对历史与 index.lock，不能自动重试".into(),
        }
    }
}

/// Hooks can emit the same diagnostic or change files outside Git's status scope.
fn reports_empty_commit(result: &GitProcessResult) -> bool {
    if result.exit_code != Some(1) || !result.stderr.is_empty() {
        return false;
    }
    // The runner fixes the C locale. Git prints this footer after refusing an empty commit.
    matches!(
        String::from_utf8_lossy(&result.stdout).lines().next_back(),
        Some(
            "nothing to commit, working tree clean"
                | "nothing to commit (use -u to show untracked files)"
                | "nothing to commit (create/copy files and use \"git add\" to track)"
                | "no changes added to commit (use \"git add\" and/or \"git commit -a\")"
                | "nothing added to commit but untracked files present (use \"git add\" to track)"
        )
    )
}
