//! Resolve targets without connecting; immutable facts bind the question and dispatch.
use super::*;
use aura_domain_common::jcs::canonical_sha256;
use aura_security_contract::{GitRemoteFactsV1, GitRemoteTargetV1};
use aura_workspace_domain::{GitPushInput, GitSyncInput};

impl GitReader {
    pub fn remote_plan(&self, kind: &str, action: &serde_json::Value) -> Result<GitRemoteFactsV1> {
        let status = self.status(&aura_workspace_domain::GitStatusInput {
            include_untracked: false,
            ..Default::default()
        })?;
        let (requested_remote, branch, destination, lease) = if kind == "git.push" {
            let input: GitPushInput = serde_json::from_value(action.clone())?;
            (
                input.remote,
                input.branch.or(status.branch.clone()),
                input.remote_branch,
                input.force_with_lease,
            )
        } else {
            let input: GitSyncInput = serde_json::from_value(action.clone())?;
            match input {
                GitSyncInput::Fetch { remote } => (remote, status.branch.clone(), None, None),
                GitSyncInput::Pull { remote, branch } => {
                    (remote, status.branch.clone(), branch, None)
                }
            }
        };
        let local_head = if kind != "git.push" {
            status.head.clone()
        } else {
            branch
                .as_ref()
                .map(|branch| -> Result<_> {
                    self.check_branch(branch)?;
                    self.oid(&format!("refs/heads/{branch}"))
                })
                .transpose()?
        };
        let upstream_remote = branch
            .as_ref()
            .map(|branch| self.config_value(&format!("branch.{branch}.remote")))
            .transpose()?
            .flatten();
        let upstream_branch = branch
            .as_ref()
            .map(|branch| self.config_value(&format!("branch.{branch}.merge")))
            .transpose()?
            .flatten()
            .and_then(|value| value.strip_prefix("refs/heads/").map(str::to_owned));
        let remote = requested_remote.or(upstream_remote.clone()).ok_or_else(|| {
            anyhow::anyhow!("git.remote_required: 请指定已配置的远端，或先为本地分支设置上游")
        })?;
        ensure!(
            !remote.is_empty()
                && remote.len() <= 1024
                && !remote.starts_with('-')
                && !remote.chars().any(char::is_control),
            "git.invalid_remote: 必须使用已配置的远端名称"
        );
        let names = self.command(&["remote"])?;
        ensure!(
            String::from_utf8(names)?.lines().any(|name| name == remote),
            "git.remote_missing: 请使用git配置中的远端名称，不能提供URL"
        );
        let remote_branch = destination.or_else(|| {
            (upstream_remote.as_deref() == Some(&remote)).then_some(upstream_branch).flatten()
        });
        let is_fetch = kind == "git.sync" && action["kind"] == "fetch";
        let remote_branch = if is_fetch {
            String::new()
        } else {
            remote_branch.ok_or_else(|| {
                anyhow::anyhow!("git.remote_branch_required: 请明确远端分支，或先设置该远端的上游")
            })?
        };
        if !is_fetch {
            self.check_branch(&remote_branch)?;
        }
        if let Some(oid) = &lease {
            ensure!(
                (oid.len() == 40 || oid.len() == 64) && oid.bytes().all(|b| b.is_ascii_hexdigit()),
                "git.exact_lease_required: force_with_lease必须为完整的期望远端提交号"
            );
        }
        let url = self.resolved_remote_url(&remote, kind == "git.push")?;
        let (remote_url, endpoint_fingerprint) = super::privacy::normalize(&url, &self.root)?;
        let repository = canonical_sha256(&self.facts()?["repository"])?;
        let (outgoing_count, outgoing_subjects) = if kind == "git.push" {
            if let (Some(head), Ok(previous)) =
                (&local_head, self.oid(&format!("refs/remotes/{remote}/{remote_branch}")))
            {
                let range = format!("{previous}..{head}");
                let count = String::from_utf8(self.command(&["rev-list", "--count", &range])?)?
                    .trim()
                    .parse()?;
                let subjects = String::from_utf8(self.command(&[
                    "log",
                    "--no-textconv",
                    "--no-ext-diff",
                    "--format=%s",
                    "-5",
                    &range,
                    "--",
                ])?)?
                .lines()
                .map(|subject| subject.chars().take(300).collect())
                .collect();
                (Some(count), subjects)
            } else {
                (None, vec![])
            }
        } else {
            (None, vec![])
        };
        Ok(GitRemoteFactsV1 {
            target: GitRemoteTargetV1 {
                repository,
                remote_url,
                endpoint_fingerprint,
                remote_branch,
            },
            remote,
            local_branch: branch,
            local_head,
            force_with_lease: lease,
            outgoing_count,
            outgoing_subjects,
        })
    }
    pub(super) fn check_branch(&self, value: &str) -> Result<()> {
        ensure!(
            !value.is_empty()
                && value.len() <= 1024
                && !value.starts_with('-')
                && !value.chars().any(char::is_control),
            "git.invalid_branch: 分支名称不合法"
        );
        ensure!(
            self.raw(&["check-ref-format".into(), format!("refs/heads/{value}")])?.exit_code
                == Some(0),
            "git.invalid_branch: 分支名称不合法"
        );
        Ok(())
    }
    fn config_value(&self, key: &str) -> Result<Option<String>> {
        let output = self.raw(&["config".into(), "--get".into(), key.into()])?;
        if output.exit_code == Some(1) {
            return Ok(None);
        }
        ensure!(output.exit_code == Some(0), "git.config_invalid: 无法读取远端分支配置");
        Ok(Some(String::from_utf8(output.stdout)?.trim_end().to_owned()))
    }
    pub(in crate::git) fn resolved_remote_url(&self, remote: &str, push: bool) -> Result<String> {
        let mut args = vec!["remote", "get-url", "--all"];
        if push {
            args.push("--push");
        }
        args.push(remote);
        let output = String::from_utf8(self.command(&args)?)?;
        let urls: Vec<_> = output.lines().collect();
        ensure!(
            urls.len() == 1,
            "git.multiple_remote_urls: 此远端有多个地址，无法绑定单一目的地批准；请在终端处理或配置单一推送地址"
        );
        Ok(urls[0].into())
    }
}
