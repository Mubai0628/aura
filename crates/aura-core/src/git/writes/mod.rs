//! Native mutations retain ordinary Git configuration and report what can be established afterwards.
mod branch;
mod commit;
mod remote;
use super::*;
use aura_execution_contract::SideEffectStateV1;

pub(crate) struct GitWriter {
    reader: GitReader,
    started: bool,
    deadline: std::time::Instant,
    on_started: Option<Box<dyn FnOnce() + Send>>,
}

#[derive(Debug)]
pub(crate) struct GitWriteInterrupted(pub String);
impl std::fmt::Display for GitWriteInterrupted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "git.outcome_unknown: {}。请查询仓库状态和历史，禁止自动重放", self.0)
    }
}
impl std::error::Error for GitWriteInterrupted {}

impl GitWriter {
    pub fn new(reader: GitReader, on_started: impl FnOnce() + Send + 'static) -> Self {
        Self {
            reader,
            started: false,
            deadline: std::time::Instant::now() + Duration::from_secs(120),
            on_started: Some(Box::new(on_started)),
        }
    }
    pub fn side_effect(&self, succeeded: bool) -> SideEffectStateV1 {
        if !self.started {
            SideEffectStateV1::NoEffect
        } else if succeeded {
            SideEffectStateV1::FullyApplied
        } else {
            SideEffectStateV1::Unknown
        }
    }
    fn native(&mut self, args: &[String]) -> Result<()> {
        let mut command = vec![
            "--no-pager".into(),
            "--literal-pathspecs".into(),
            "-c".into(),
            "color.ui=false".into(),
            "-c".into(),
            "protocol.allow=never".into(),
        ];
        command.extend_from_slice(args);
        let remaining = self.deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(GitWriteInterrupted("Git 写入超过时限".into()).into());
        }
        let reader = &self.reader;
        let started = &mut self.started;
        let lease = &mut self.on_started;
        let result = reader.runner.run_native_at_start(
            &reader.root,
            &command,
            &reader.cancel,
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
            return Err(GitWriteInterrupted(format!(
                "Git 已停止，写入结果未确认（{}）",
                result.reason
            ))
            .into());
        }
        if result.exit_code != Some(0) {
            let mut diagnostic = result.stderr;
            diagnostic.extend_from_slice(b"\n");
            diagnostic.extend(result.stdout);
            anyhow::bail!("{}", failure(args.first().map_or("write", String::as_str), &diagnostic));
        }
        Ok(())
    }
    fn refresh(&mut self) -> Result<()> {
        self.reader.deadline = std::time::Instant::now() + Duration::from_secs(30);
        let result = config::read_configuration(&self.reader);
        let (configuration, programs) = result.map_err(|error| {
            GitWriteInterrupted(format!("Git 返回成功，但无法核对写入后状态：{error}"))
        })?;
        self.reader.config = configuration;
        self.reader.programs = programs;
        Ok(())
    }
}

fn failure(command: &str, stderr: &[u8]) -> String {
    let diagnostic = config::redact(&String::from_utf8_lossy(stderr))
        .chars()
        .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\r' | '\t'))
        .scan(0, |bytes, ch| {
            *bytes += ch.len_utf8();
            (*bytes <= 7600).then_some(ch)
        })
        .collect::<String>();
    let lower = diagnostic.to_ascii_lowercase();
    let (code, explanation) = if lower.contains("index.lock") || lower.contains("cannot lock ref") {
        ("locked", "Git 文件被锁定；请检查其他 Git 操作，Aura 不会删除锁文件")
    } else if lower.contains("would be overwritten") || lower.contains("local changes") {
        ("local_changes", "本地改动阻止切换；请先处理改动，Aura 不会强制丢弃")
    } else if lower.contains("unmerged")
        || lower.contains("unresolved conflict")
        || lower.contains("middle of a merge")
    {
        ("operation_in_progress", "合并或其他操作尚未完成；请先在终端处理冲突或完成操作")
    } else if lower.contains("hook") {
        ("hook_rejected", "Git hook 拒绝了操作；请查看输出并修复，Aura 不会跳过 hook")
    } else if lower.contains("gpg") || lower.contains("signing") {
        ("signing_failed", "Git 签名失败；请核对本机签名配置，Aura 不会关闭签名")
    } else if lower.contains("nothing to commit") || lower.contains("no changes added") {
        ("nothing_to_commit", "没有可提交的暂存改动")
    } else {
        ("write_failed", "Git 写入未成功；请查看 hook、签名或仓库错误详情")
    };
    format!("git.{code}: {explanation}（{command}）。{diagnostic}")
}
