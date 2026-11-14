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
        let arguments = |error: serde_json::Error| {
            anyhow::anyhow!(
                "git.invalid_arguments: 这次远端操作的参数无法解析（{}），没有执行。请按工具说明改正参数后再试。",
                config::shown(&error.to_string())
            )
        };
        let (requested_remote, branch, destination, lease) = if kind == "git.push" {
            let input: GitPushInput = serde_json::from_value(action.clone()).map_err(arguments)?;
            (
                input.remote,
                input.branch.or(status.branch.clone()),
                input.remote_branch,
                input.force_with_lease,
            )
        } else {
            let input: GitSyncInput = serde_json::from_value(action.clone()).map_err(arguments)?;
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
            anyhow::anyhow!(
                "git.remote_required: 没有给出 remote，当前分支也没有设置上游，不知道要连接哪个远端；没有连接远端。请在 remote 里写明已配置的远端名称，或先为本地分支设置上游。"
            )
        })?;
        ensure!(
            !remote.is_empty()
                && remote.len() <= 1024
                && !remote.starts_with('-')
                && !remote.chars().any(char::is_control),
            "git.invalid_remote: remote「{}」不是合法的远端名称（不能为空、不能以 - 开头、不能含控制字符），没有连接远端。请使用 Git 配置里已有的远端名称。",
            config::shown(&remote)
        );
        let names = String::from_utf8(self.command(&["remote"])?)
            .map_err(|_| config::unreadable_output("remote_format", "远端列表"))?;
        ensure!(
            names.lines().any(|name| name == remote),
            "git.remote_missing: 仓库里没有名为「{}」的远端，没有连接远端。请使用 Git 配置里已有的远端名称（不能直接给 URL）。",
            config::shown(&remote)
        );
        let remote_branch = destination.or_else(|| {
            (upstream_remote.as_deref() == Some(&remote)).then_some(upstream_branch).flatten()
        });
        let is_fetch = kind == "git.sync" && action["kind"] == "fetch";
        let remote_branch = if is_fetch {
            String::new()
        } else {
            remote_branch.ok_or_else(|| {
                anyhow::anyhow!(
                    "git.remote_branch_required: 没有给出远端分支，当前分支在这个远端也没有上游；没有连接远端。请写明远端分支（git.push 用 remote_branch，git.sync 的 pull 用 branch），或先设置上游。"
                )
            })?
        };
        if !is_fetch {
            self.check_branch(&remote_branch)?;
        }
        if let Some(oid) = &lease {
            ensure!(
                (oid.len() == 40 || oid.len() == 64) && oid.bytes().all(|b| b.is_ascii_hexdigit()),
                "git.exact_lease_required: force_with_lease 必须是远端分支现在应有的完整提交号（40 或 64 位十六进制），没有推送。请先用 git.sync 的 fetch 核对远端分支后再试。"
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
                let count = String::from_utf8(self.command(&["rev-list", "--count", &range])?)
                    .ok()
                    .and_then(|count| count.trim().parse().ok())
                    .ok_or_else(|| config::unreadable_output("count_format", "待推送提交数"))?;
                let subjects = String::from_utf8(self.command(&[
                    "log",
                    "--no-textconv",
                    "--no-ext-diff",
                    "--format=%s",
                    "-5",
                    &range,
                    "--",
                ])?)
                .map_err(|_| config::unreadable_output("log_format", "待推送提交的说明"))?
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
                && !value.chars().any(char::is_control)
                && self.raw(&["check-ref-format".into(), format!("refs/heads/{value}")])?.exit_code
                    == Some(0),
            "{}",
            crate::git::config::invalid_branch(value)
        );
        Ok(())
    }
    fn config_value(&self, key: &str) -> Result<Option<String>> {
        let output = self.raw(&["config".into(), "--get".into(), key.into()])?;
        if output.exit_code == Some(1) {
            return Ok(None);
        }
        ensure!(
            output.exit_code == Some(0),
            "git.config_invalid: 读取分支的上游配置（{}）没有成功，没有连接远端。请在终端运行 git config --get {} 查看并修复。",
            config::shown(key),
            config::shown(key)
        );
        let value = String::from_utf8(output.stdout)
            .map_err(|_| config::unreadable_output("config_format", "上游配置"))?;
        Ok(Some(value.trim_end().to_owned()))
    }
    pub(in crate::git) fn resolved_remote_url(&self, remote: &str, push: bool) -> Result<String> {
        let mut args = vec!["remote", "get-url", "--all"];
        if push {
            args.push("--push");
        }
        args.push(remote);
        let output = String::from_utf8(self.command(&args)?)
            .map_err(|_| config::unreadable_output("remote_url_format", "远端地址"))?;
        let urls: Vec<_> = output.lines().collect();
        ensure!(
            urls.len() == 1,
            "git.multiple_remote_urls: 此远端有多个地址，无法绑定单一目的地批准；请在终端处理或配置单一推送地址"
        );
        Ok(urls[0].into())
    }
}
