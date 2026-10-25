use super::*;
use aura_workspace_domain::{
    GIT_NATIVE_EXECUTION_NOTICE, GitBranchInput, GitBranchResultV1, GitStatusInput,
};

impl GitWriter {
    pub fn branch(&mut self, input: &GitBranchInput) -> Result<GitBranchResultV1> {
        let (name, create, switch, start) = match input {
            GitBranchInput::Create { name, switch, start } => {
                (name, true, *switch, start.as_deref())
            }
            GitBranchInput::Switch { name } => (name, false, true, None),
        };
        ensure!(
            !name.is_empty()
                && name.len() <= 1024
                && !name.starts_with('-')
                && !name.chars().any(char::is_control),
            "git.invalid_branch: 分支名称不合法"
        );
        let reference = format!("refs/heads/{name}");
        let checked = self.reader.raw(&["check-ref-format".into(), reference.clone()])?;
        ensure!(checked.exit_code == Some(0), "git.invalid_branch: 分支名称不合法");
        let start = start.map(|revision| self.reader.oid(revision)).transpose()?;
        if !create {
            let exists = self.reader.raw(&[
                "show-ref".into(),
                "--verify".into(),
                "--hash".into(),
                reference.clone(),
            ])?;
            ensure!(
                exists.exit_code == Some(0),
                "git.branch_missing: 本地分支不存在，请用git.status核对分支"
            );
        }
        let mut command = if switch {
            let mut args = vec!["switch".into(), "--no-guess".into()];
            if create {
                args.extend(["-c".into(), name.clone()]);
            } else {
                args.extend(["--".into(), name.clone()]);
            }
            args
        } else {
            vec!["branch".into(), "--".into(), name.clone()]
        };
        if let Some(start) = start {
            command.push(start);
        }
        let before = self
            .reader
            .status(&GitStatusInput { include_untracked: false, ..Default::default() })?;
        self.native(&command)?;
        self.refresh()?;
        let observed = (|| -> Result<_> {
            let status = self.reader.status(&GitStatusInput::default())?;
            let head = if switch {
                ensure!(status.branch.as_deref() == Some(name), "当前分支与请求不符");
                status.head.clone()
            } else {
                Some(self.reader.oid(&reference)?)
            };
            Ok(GitBranchResultV1 {
                previous_branch: before.branch,
                previous_head: before.head,
                branch: name.clone(),
                head,
                created: create,
                switched: switch,
                status,
                execution_notice: GIT_NATIVE_EXECUTION_NOTICE.into(),
            })
        })();
        observed.map_err(|error| {
            GitWriteInterrupted(format!("Git 返回成功，但分支回执未确认：{error}")).into()
        })
    }
}
