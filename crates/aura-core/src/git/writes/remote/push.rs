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
            anyhow::anyhow!("git.local_branch_required: 分离HEAD时请指定本地分支")
        })?;
        let expected = approved
            .local_head
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("git.local_branch_unborn: 本地分支尚无提交"))?;
        let reference = format!("refs/heads/{}", approved.target.remote_branch);
        let before = self.remote_oid(&approved.remote, &reference)?;
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
                "Git返回成功，但指定分支的推送回执缺失；请核对远端".into(),
            )
            .into());
        };
        let after = self.remote_oid(&approved.remote, &reference).map_err(|_| {
            GitWriteInterrupted("推送已返回，但远端回执未确认；请查询远端，禁止自动重放".into())
        })?;
        if after.as_deref() != Some(expected) {
            return Err(GitWriteInterrupted(
                "远端提交与本次推送不符；请查询远端，禁止自动重放".into(),
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
        let text = String::from_utf8(result.stdout)
            .map_err(|_| anyhow::anyhow!("git.invalid_remote_receipt"))?;
        let mut found = None;
        for line in text.lines() {
            let Some((oid, name)) = line.split_once('\t') else {
                anyhow::bail!("git.invalid_remote_receipt");
            };
            ensure!(
                name == reference
                    && found.is_none()
                    && (oid.len() == 40 || oid.len() == 64)
                    && oid.bytes().all(|b| b.is_ascii_hexdigit()),
                "git.invalid_remote_receipt"
            );
            found = Some(oid.to_owned());
        }
        Ok(found)
    }
}
