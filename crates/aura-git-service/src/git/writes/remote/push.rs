use super::*;
use aura_workspace_domain::{GitPushInput, GitRefUpdateV1};

impl GitWriter {
    pub(super) fn push_remote(
        &mut self,
        action: &serde_json::Value,
        approved: &GitRemoteFactsV1,
    ) -> Result<Vec<GitRefUpdateV1>> {
        let input: GitPushInput = serde_json::from_value(action.clone())?;
        let branch = approved.local_branch.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "git.local_branch_required: 当前 HEAD 不在任何分支上（分离 HEAD），又没有给出 branch，推送没有执行。请在 branch 里写明要推送的本地分支后再试。"
            )
        })?;
        let expected = approved.local_head.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "git.local_branch_unborn: 本地分支「{}」还没有提交，没有可推送的内容，推送没有执行。请先提交后再推送。",
                config::shown(branch)
            )
        })?;
        let reference = format!("refs/heads/{}", approved.target.remote_branch);
        let before = self.remote_oid(&approved.remote, &reference).map_err(|error| {
            if error.is::<GitWriteInterrupted>() {
                GitWriteInterrupted(
                    "推送前查询远端分支没有成功（联网失败、被停止或超时），推送命令还没有运行"
                        .into(),
                )
                .into()
            } else {
                error
            }
        })?;
        let mut args = vec![
            "push".into(),
            "--porcelain".into(),
            "--no-follow-tags".into(),
            "--no-mirror".into(),
            "--recurse-submodules=no".into(),
        ];
        if input.set_upstream {
            args.push("--set-upstream".into());
        }
        if let Some(lease) = &approved.force_with_lease {
            args.push(format!("--force-with-lease={reference}:{lease}"));
        }
        args.extend([
            "--".into(),
            approved.remote.clone(),
            format!("refs/heads/{branch}:{reference}"),
        ]);
        let output = self.network_command(&args)?;
        let porcelain = String::from_utf8_lossy(&output.stdout);
        let record = porcelain.lines().find(|line| {
            line.split('\t').nth(1) == Some(&format!("refs/heads/{branch}:{reference}"))
        });
        let Some(record) = record
            .filter(|line| matches!(line.as_bytes().first(), Some(b' ' | b'=' | b'*' | b'+')))
        else {
            return Err(GitWriteInterrupted(
                "git push 已成功退出，但输出里没有这个分支的推送结果，无法确认推送是否完成".into(),
            )
            .into());
        };
        let after = self.remote_oid(&approved.remote, &reference).map_err(|_| {
            GitWriteInterrupted(
                "git push 已成功退出，推送很可能已经完成，但随后查询远端分支没有成功，无法确认远端指向的提交"
                    .into(),
            )
        })?;
        if after.as_deref() != Some(expected) {
            return Err(GitWriteInterrupted(
                "git push 已成功退出，但随后远端分支指向的提交不是这次推送的提交（可能有其他人同时推送），无法确认推送结果"
                    .into(),
            )
            .into());
        }
        Ok(vec![GitRefUpdateV1 {
            reference,
            before,
            after,
            result: if record.starts_with('=') { "up_to_date".into() } else { "pushed".into() },
        }])
    }
    fn remote_oid(&mut self, remote: &str, reference: &str) -> Result<Option<String>> {
        self.reader.deadline = std::time::Instant::now() + Duration::from_secs(30);
        // A distinct pushurl must be queried too; querying the fetch URL would report
        // another repository and could connect to a destination the card never named.
        let url = self.reader.resolved_remote_url(remote, true)?;
        let result = self.network_command(&[
            "ls-remote".into(),
            "--refs".into(),
            "--".into(),
            url,
            reference.into(),
        ])?;
        let receipt = || config::unreadable_output("invalid_remote_receipt", "远端分支信息");
        let text = String::from_utf8(result.stdout).map_err(|_| receipt())?;
        let mut found = None;
        for line in text.lines() {
            let Some((oid, name)) = line.split_once('\t') else {
                return Err(receipt());
            };
            if name != reference
                || found.is_some()
                || !matches!(oid.len(), 40 | 64)
                || !oid.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(receipt());
            }
            found = Some(oid.to_owned());
        }
        Ok(found)
    }
}
