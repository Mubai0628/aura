use super::*;
use aura_workspace_domain::{
    GIT_NATIVE_EXECUTION_NOTICE, GitCommitInput, GitCommitResultV1, GitLogInput, GitStatusInput,
};

impl GitWriter {
    pub fn commit(&mut self, input: &GitCommitInput) -> Result<GitCommitResultV1> {
        ensure!(
            !input.message.trim().is_empty()
                && input.message.len() <= 65536
                && !input.message.contains('\0'),
            "git.invalid_message: 提交说明必须非空，最多65536字节且不能含NUL"
        );
        ensure!(
            input.staged_only == input.paths.is_empty(),
            "git.invalid_selection: 请提供要提交的路径，或仅选 staged_only，不能同时选择两者"
        );
        let mut pathspec = Vec::new();
        paths(&mut pathspec, &input.paths)?;
        let before = self
            .reader
            .status(&GitStatusInput { include_untracked: false, ..Default::default() })?
            .head;
        let mutation = (|| -> Result<()> {
            if !input.staged_only {
                let mut add = vec!["add".into(), "--all".into()];
                add.extend(pathspec.clone());
                self.native(&add)?;
            }
            let mut command = vec!["commit".into(), "--message".into(), input.message.clone()];
            if !input.staged_only {
                command.push("--only".into());
                command.extend(pathspec);
            }
            self.native(&command)
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
            let head = status.head.clone().ok_or_else(|| anyhow::anyhow!("提交后没有HEAD"))?;
            let log = self.reader.log(&GitLogInput {
                limit: 1,
                start: None,
                paths: vec![],
                commit: Some(head.clone()),
            })?;
            let commit =
                log.commits.into_iter().next().ok_or_else(|| anyhow::anyhow!("提交回执缺失"))?;
            ensure!(
                Some(&head) != before.as_ref() && commit.parents.first() == before.as_ref(),
                "HEAD在执行期间发生了其他变更"
            );
            Ok(GitCommitResultV1 {
                commit,
                branch: status.branch.clone(),
                status,
                execution_notice: GIT_NATIVE_EXECUTION_NOTICE.into(),
            })
        })();
        observed.map_err(|error| {
            GitWriteInterrupted(format!("Git 返回成功，但提交回执未确认：{error}")).into()
        })
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
                        "中断后 HEAD 未移动（{}）；index.lock 若存在将保留，请检查后再操作",
                        current.unwrap_or("尚无提交")
                    )
                } else {
                    format!(
                        "中断后 HEAD 已变化：{} → {}；请核对历史与 index.lock，不能据此自动重试",
                        before.unwrap_or("尚无提交"),
                        current.unwrap_or("尚无提交")
                    )
                }
            }
            _ => "中断后 HEAD 无法确认；请手动核对历史与 index.lock，不能自动重试".into(),
        }
    }
}
