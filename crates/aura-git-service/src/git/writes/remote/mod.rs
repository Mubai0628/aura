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
            "git.remote_changed: 批准之后仓库、分支或远端已经改变，这次操作没有执行，也没有连接远端。请重新调用，按当前目标重新批准。"
        );
        let refs = if kind == "git.push" {
            self.push_remote(action, approved)?
        } else {
            self.sync_remote(action, approved)?
        };
        self.refresh()?;
        let status = self.reader.status(&GitStatusInput::default()).map_err(|error| {
            GitWriteInterrupted(format!(
                "远端操作已经完成，但之后读不到本地仓库状态，无法核对结果（{}）",
                brief(&error)
            ))
        })?;
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
            return Err(GitWriteInterrupted(
                "远端操作用完了 120 秒的时限，后面的步骤没有执行，前面的步骤可能已经改变了本地或远端".into(),
            )
            .into());
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
                "Git 联网在途中出错、被停止、超时或输出超过上限，远端可能已经接收了这次操作".into(),
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
        "git.lease_mismatch: 远端分支的提交与 force_with_lease 给出的不符，推送被拒绝，远端没有改变。请先用 git.sync 的 fetch 核对远端，再决定是否推送；Aura 不会强制推送。"
    } else if text.contains("non-fast-forward") || text.contains("fetch first") {
        "git.non_fast_forward: 远端有本地没有的提交，推送被拒绝，远端没有改变。请先用 git.sync 取回远端的提交，并在终端处理分歧后再推送。"
    } else if text.contains("not possible to fast-forward") || text.contains("diverging") {
        "git.cannot_fast_forward: 本地分支和远端已经分叉，无法快进，本地分支没有改变（远端的提交已经取回）。请在终端合并或变基；Aura 不会自动处理分歧。"
    } else if text.contains("authentication")
        || text.contains("401")
        || text.contains("403")
        || text.contains("could not read username")
    {
        "git.authentication_failed: Git 认证失败，远端没有接受这次连接。请在终端或系统凭据管理里检查登录后再试。"
    } else if text.contains("remote rejected")
        || text.contains("hook declined")
        || text.contains("pre-push hook")
    {
        "git.remote_rejected: 远端或 Git hook 拒绝了这次操作，远端没有接受它。请在终端查看原因并修复；Aura 不会绕过 hook。"
    } else {
        return GitWriteInterrupted(
            "Git 远端连接或操作失败，原因无法从输出判断，远端可能已经接收了这次操作；请在终端核对远端、网络、凭据与 hook"
                .into(),
        )
        .into();
    };
    anyhow::anyhow!(message)
}
