//! Network execution is Native. A returned error never authorizes automatic replay.
mod push;
mod sync;
use super::*;
use aura_security_contract::GitRemoteFactsV1;
use aura_workspace_domain::{GIT_NATIVE_EXECUTION_NOTICE, GitRemoteResultV1, GitStatusInput};

impl GitWriter {
    pub fn remote(
        &mut self,
        kind: &str,
        action: &serde_json::Value,
        approved: &GitRemoteFactsV1,
    ) -> Result<GitRemoteResultV1> {
        ensure!(
            self.reader.remote_plan(kind, action)? == *approved,
            "git.remote_changed: 仓库、分支或远端已改变，请重新发起操作"
        );
        let refs = if kind == "git.push" {
            self.push_remote(action, approved)?
        } else {
            self.sync_remote(action, approved)?
        };
        self.refresh()?;
        let status = self
            .reader
            .status(&GitStatusInput::default())
            .map_err(|_| GitWriteInterrupted("操作已结束，但本地状态回执无法核对".into()))?;
        Ok(GitRemoteResultV1 {
            kind: if kind == "git.push" {
                "push".into()
            } else {
                action["kind"].as_str().unwrap_or("sync").into()
            },
            remote: approved.remote.clone(),
            remote_url: approved.target.remote_url.clone(),
            local_branch: approved.local_branch.clone(),
            remote_branch: (!approved.target.remote_branch.is_empty())
                .then(|| approved.target.remote_branch.clone()),
            refs,
            status,
            execution_notice: GIT_NATIVE_EXECUTION_NOTICE.into(),
        })
    }
    fn network_command(&mut self, args: &[String]) -> Result<GitProcessResult> {
        let mut command = vec![
            "--no-pager".into(),
            "--literal-pathspecs".into(),
            "-c".into(),
            "color.ui=false".into(),
        ];
        command.extend_from_slice(args);
        let remaining = self.deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(GitWriteInterrupted("远端操作超过时限，结果未确认".into()).into());
        }
        let started = &mut self.started;
        let lease = &mut self.on_started;
        let result = self.reader.runner.run_remote_at_start(
            &self.reader.root,
            &command,
            &self.reader.cancel,
            (remaining, 256 * 1024),
            &mut || {
                *started = true;
                if let Some(release) = lease.take() {
                    release();
                }
            },
        )?;
        if !result.stopped {
            return Err(StopUnconfirmed.into());
        }
        if result.reason != "exited" {
            if !self.started && result.reason == "cancelled" {
                return Err(GitCancelled.into());
            }
            return Err(GitWriteInterrupted(
                "联网已中断或输出超过上限，远端可能已接收操作；请查询远端与本地状态，禁止自动重放"
                    .into(),
            )
            .into());
        }
        if result.exit_code != Some(0) {
            return Err(remote_failure(&result));
        }
        Ok(result)
    }
}

fn remote_failure(result: &GitProcessResult) -> anyhow::Error {
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    )
    .to_ascii_lowercase();
    // Git diagnostics can contain userinfo, query tokens and helper output. Keep only the
    // classified explanation, never the raw remote stderr/stdout in receipts or journals.
    let message = if text.contains("stale info") {
        "git.lease_mismatch: 远端提交与精确租约不符；请重新fetch核对，Aura不会强制推送"
    } else if text.contains("non-fast-forward") || text.contains("fetch first") {
        "git.non_fast_forward: 远端含本地没有的提交；请先fetch并在终端处理分歧"
    } else if text.contains("not possible to fast-forward") || text.contains("diverging") {
        "git.cannot_fast_forward: 无法快进；请在终端合并或变基，Aura不会自动处理分歧"
    } else if text.contains("authentication")
        || text.contains("401")
        || text.contains("403")
        || text.contains("could not read username")
    {
        "git.authentication_failed: Git认证失败；请在终端或系统凭据界面检查登录后重试"
    } else if text.contains("remote rejected")
        || text.contains("hook declined")
        || text.contains("pre-push hook")
    {
        "git.remote_rejected: 远端或Git hook拒绝了操作；请在终端查看并修复，Aura不会绕过hook"
    } else {
        return GitWriteInterrupted("Git远端连接或操作失败；远端可能已经接收操作，请在终端核对远端、网络、凭据与hook后查询结果".into()).into();
    };
    anyhow::anyhow!(message)
}
