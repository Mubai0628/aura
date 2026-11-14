use super::*;
use aura_workspace_domain::{GitCommitV1, GitDiffTargetV1, GitLogInput, GitLogResultV1};
impl GitReader {
    pub(super) fn log(&self, input: &GitLogInput) -> Result<GitLogResultV1> {
        ensure!(
            input.limit > 0 && input.limit <= 200,
            "git.log_limit_or_target: limit 要在 1 到 200 之间，这次是 {}。请改用这个范围内的值再读。",
            input.limit
        );
        ensure!(
            !(input.commit.is_some() && input.start.is_some()),
            "git.log_limit_or_target: commit 和 start 只能给一个：commit 读单个提交及其改动的文件，start 从这个提交往前列出历史。"
        );
        let specified = input.commit.as_deref().or(input.start.as_deref());
        let mut args = vec![
            "log".into(),
            "-z".into(),
            "--no-show-signature".into(),
            "--no-patch".into(),
            "--no-ext-diff".into(),
            "--no-textconv".into(),
            "--format=%H%x00%P%x00%an%x00%aI%x00%s%x00%B".into(),
            format!("--max-count={}", if input.commit.is_some() { 1 } else { input.limit + 1 }),
        ];
        if let Some(commit) = specified {
            args.push(self.oid(commit)?);
        } else {
            let head =
                self.raw(&["rev-parse", "--verify", "--quiet", "HEAD"].map(str::to_string))?;
            if head.exit_code == Some(1) {
                return Ok(GitLogResultV1 { commits: vec![], truncated: false });
            }
            ensure!(head.exit_code == Some(0), "{}", super::config::failure(&head.stderr));
        }
        paths(&mut args, &input.paths)?;
        let bytes = self.command_owned(args)?;
        let mut fields = bytes.split(|byte| *byte == 0).collect::<Vec<_>>();
        if fields.last() == Some(&&b""[..]) {
            fields.pop();
        }
        let log_format = || super::config::unreadable_output("log_format", "提交记录");
        if !fields.len().is_multiple_of(6) {
            return Err(log_format());
        }
        let mut commits = vec![];
        for row in fields.chunks_exact(6).take(input.limit as usize) {
            let text = row
                .iter()
                .map(|field| std::str::from_utf8(field))
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|_| log_format())?;
            let files = if input.commit.is_some() {
                let mut args = super::diff::diff_arguments(
                    self,
                    &GitDiffTargetV1::Commit { commit: text[0].into() },
                )?;
                args.extend(["--numstat".into(), "-z".into(), "--no-textconv".into()]);
                paths(&mut args, &input.paths)?;
                let mut files = super::diff::parse_numstat(&self.command_owned(args)?)?;
                self.annotate_stats(&mut files)?;
                files
            } else {
                vec![]
            };
            commits.push(GitCommitV1 {
                commit: text[0].into(),
                parents: text[1].split_whitespace().map(str::to_string).collect(),
                author: text[2].into(),
                authored_at: text[3].into(),
                subject: text[4].into(),
                body: text[5].into(),
                files,
            });
        }
        Ok(GitLogResultV1 { commits, truncated: fields.len() / 6 > input.limit as usize })
    }
}
