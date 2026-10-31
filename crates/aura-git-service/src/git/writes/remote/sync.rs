use super::*;
use aura_workspace_domain::{GitRefUpdateV1, GitSyncInput};
use std::collections::BTreeMap;

impl GitWriter {
    pub(super) fn sync_remote(
        &mut self,
        action: &serde_json::Value,
        approved: &GitRemoteFactsV1,
    ) -> Result<Vec<GitRefUpdateV1>> {
        let input: GitSyncInput = serde_json::from_value(action.clone())?;
        let before = self.reader.remote_refs()?;
        self.network_command(&[
            "fetch".into(),
            "--no-recurse-submodules".into(),
            "--".into(),
            approved.remote.clone(),
        ])?;
        self.reader.deadline = std::time::Instant::now() + Duration::from_secs(30);
        if matches!(input, GitSyncInput::Pull { .. }) {
            let reference = format!("refs/heads/{}", approved.target.remote_branch);
            // Fetch an explicit branch into FETCH_HEAD, even if the user's fetch refspec
            // excludes it. Never run git pull (which may inherit rebase/autostash settings).
            self.network_command(&[
                "fetch".into(),
                "--no-recurse-submodules".into(),
                "--".into(),
                approved.remote.clone(),
                reference,
            ])?;
            self.reader.deadline = std::time::Instant::now() + Duration::from_secs(30);
            let fetched = self.reader.oid("FETCH_HEAD")?;
            let status = self
                .reader
                .status(&GitStatusInput { include_untracked: false, ..Default::default() })?;
            ensure!(
                status.branch == approved.local_branch && status.head == approved.local_head,
                "git.branch_changed: fetch期间本地分支已改变；请核对后重新发起pull"
            );
            // This local step remains Native and preserves hooks/filters. Explicitly
            // disable autostash; divergence and overlapping local edits must fail.
            self.network_command(&[
                "merge".into(),
                "--ff-only".into(),
                "--no-autostash".into(),
                "--".into(),
                fetched.clone(),
            ])?;
            self.reader.deadline = std::time::Instant::now() + Duration::from_secs(30);
            ensure!(
                self.reader.oid("HEAD")? == fetched
                    || self
                        .reader
                        .raw(&[
                            "merge-base".into(),
                            "--is-ancestor".into(),
                            fetched,
                            "HEAD".into()
                        ])?
                        .exit_code
                        == Some(0),
                "git.outcome_unknown: 无法确认快进结果，请查询仓库状态"
            );
        }
        let after = self.reader.remote_refs()?;
        let keys: std::collections::BTreeSet<_> =
            before.keys().chain(after.keys()).cloned().collect();
        Ok(keys
            .into_iter()
            .filter_map(|reference| {
                let old = before.get(&reference).cloned();
                let new = after.get(&reference).cloned();
                (old != new).then_some(GitRefUpdateV1 {
                    reference,
                    before: old,
                    after: new,
                    result: "fetched".into(),
                })
            })
            .collect())
    }
}
impl GitReader {
    fn remote_refs(&self) -> Result<BTreeMap<String, String>> {
        let bytes = self.command(&[
            "for-each-ref",
            "--format=%(refname)%09%(objectname)",
            "refs/remotes",
            "refs/heads",
            "refs/tags",
        ])?;
        String::from_utf8(bytes)?
            .lines()
            .map(|line| {
                let (name, oid) = line
                    .split_once('\t')
                    .ok_or_else(|| anyhow::anyhow!("git.invalid_ref_receipt"))?;
                Ok((name.to_owned(), oid.to_owned()))
            })
            .collect()
    }
}
